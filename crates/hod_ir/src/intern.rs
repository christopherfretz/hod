//! Hash-consing support: a deterministic hasher and an id-only hash table.
//!
//! The tables of `spec/ir.md` §3 and §4 keep their entries in arenas and
//! variable-length parts in shared pools, so an entry cannot be a
//! `HashMap` key without storing it twice. [`InternTable`] stores only
//! `(hash, id)` pairs; the owner supplies the equality test against its
//! arena. Nothing here depends on addresses or random state, so ids are
//! assigned deterministically.

use std::fmt::Debug;
use std::ops::Deref;
use std::sync::Arc;

use crate::error::{IrErrorKind, IrResult, err};

/// An FxHash-style multiplicative hasher over 64-bit words.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Hasher64 {
    state: u64,
}

impl Hasher64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;

    /// A hasher seeded with `tag` (the entry kind).
    #[inline]
    pub(crate) fn new(tag: u64) -> Self {
        let mut h = Self { state: 0 };
        h.word(tag);
        h
    }

    /// Mixes one word in.
    #[inline]
    pub(crate) fn word(&mut self, w: u64) {
        self.state = (self.state.rotate_left(5) ^ w).wrapping_mul(Self::K);
    }

    /// Mixes two 32-bit values in.
    #[inline]
    pub(crate) fn pair(&mut self, a: u32, b: u32) {
        self.word(u64::from(a) | (u64::from(b) << 32));
    }

    /// Mixes a byte string in, length included.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.word(bytes.len() as u64);
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let mut w = [0u8; 8];
            w.copy_from_slice(chunk);
            self.word(u64::from_le_bytes(w));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut w = [0u8; 8];
            w[..rest.len()].copy_from_slice(rest);
            self.word(u64::from_le_bytes(w));
        }
    }

    /// The 32-bit hash. The multiplication pushes entropy upward, so the
    /// high half is the well-mixed one.
    #[inline]
    pub(crate) fn finish(self) -> u32 {
        (self.state >> 32) as u32
    }
}

/// One slot: the entry's hash and id. `id == EMPTY` marks a free slot.
#[derive(Clone, Copy, Debug)]
struct Slot {
    hash: u32,
    id: u32,
}

const EMPTY: u32 = u32::MAX;

/// The largest number of entries a table with `u32` ids can hold
/// (`u32::MAX` itself is the free-slot marker).
pub(crate) const MAX_ENTRIES: usize = (u32::MAX - 1) as usize;

/// The id for the next entry of an arena holding `len` entries.
pub(crate) fn next_id(len: usize, what: &str) -> IrResult<u32> {
    if len >= MAX_ENTRIES {
        return err(IrErrorKind::TableFull, format!("the {what} table is full"));
    }
    Ok(len as u32)
}

/// The start index for `extra` more elements in a pool holding `len`:
/// pool positions are `u32`.
pub(crate) fn pool_start(len: usize, extra: usize, what: &str) -> IrResult<u32> {
    match len.checked_add(extra) {
        Some(end) if end <= MAX_ENTRIES => Ok(len as u32),
        _ => err(IrErrorKind::TableFull, format!("the {what} pool is full")),
    }
}

/// What a table entry must be to live in a [`Seg`].
pub trait Entry: Clone + Debug + Send + Sync + 'static {}

impl<T: Clone + Debug + Send + Sync + 'static> Entry for T {}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::NoLayer {}
    impl Sealed for super::Layered {}
}

/// How a module's tables are kept (`spec/ir.md` §5.1, "Reading bodies in
/// parallel"). [`NoLayer`]: one vector and one hash table per table, as
/// a module is built, read and compiled; the type parameter of every
/// table and of [`crate::Module`] defaults to it. [`Layered`]: a shared
/// base, copied once from a module, and a local layer above it, for the
/// forks that read bodies on other threads. Code generic over the two is
/// instantiated twice, so the one-thread path is the plain tables' code:
/// no base pointer, no test.
pub trait Layering:
    sealed::Sealed + Clone + Copy + Debug + Default + Send + Sync + 'static
{
    /// A table's layer: nothing, or where its base ends.
    #[doc(hidden)]
    type Layer<T: Entry>: Clone + Debug + Default + Send + Sync;
    /// A hash table's base: nothing, or the shared one.
    #[doc(hidden)]
    type Below: Clone + Debug + Default + Send + Sync;
    /// What a module only reads: owned, or shared with the other forks.
    #[doc(hidden)]
    type Own<T: Entry + Default>: Clone + Debug + Default + Send + Sync + Deref<Target = T>;
    /// What a fork knows of its origin; in a module, a count of the
    /// changes to what forks read.
    #[doc(hidden)]
    type Origin<T: Entry>: Clone + Debug + Default + Send + Sync;

    #[doc(hidden)]
    fn layer<T: Entry>(layer: &Self::Layer<T>) -> Option<&Layer<T>>;
    #[doc(hidden)]
    fn layer_mut<T: Entry>(layer: &mut Self::Layer<T>) -> Option<&mut Layer<T>>;
    #[doc(hidden)]
    fn below(below: &Self::Below) -> Option<&InternTable>;
    #[doc(hidden)]
    fn own_mut<T: Entry + Default>(own: &mut Self::Own<T>) -> &mut T;
    #[doc(hidden)]
    fn origin<T: Entry>(origin: &Self::Origin<T>) -> Option<&T>;
    #[doc(hidden)]
    fn origin_mut<T: Entry>(origin: &mut Self::Origin<T>) -> Option<&mut T>;
    /// Counts a change to what forks read (a module) or does nothing (a
    /// fork, which says so in its origin).
    #[doc(hidden)]
    fn count_edit<T: Entry>(origin: &mut Self::Origin<T>);
    #[doc(hidden)]
    fn edits<T: Entry>(origin: &Self::Origin<T>) -> u32;
}

/// The tables of a module: one layer ([`Layering`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoLayer;

/// The tables of a fork: a shared base and a local layer ([`Layering`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct Layered;

/// [`NoLayer`]'s owned value: the value itself.
#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct Inline<T>(pub(crate) T);

impl<T> Deref for Inline<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl Layering for NoLayer {
    type Layer<T: Entry> = ();
    type Below = ();
    type Own<T: Entry + Default> = Inline<T>;
    type Origin<T: Entry> = u32;

    #[inline]
    fn layer<T: Entry>((): &()) -> Option<&Layer<T>> {
        None
    }

    #[inline]
    fn layer_mut<T: Entry>((): &mut ()) -> Option<&mut Layer<T>> {
        None
    }

    #[inline]
    fn below((): &()) -> Option<&InternTable> {
        None
    }

    #[inline]
    fn own_mut<T: Entry + Default>(own: &mut Inline<T>) -> &mut T {
        &mut own.0
    }

    #[inline]
    fn origin<T: Entry>(_: &u32) -> Option<&T> {
        None
    }

    #[inline]
    fn origin_mut<T: Entry>(_: &mut u32) -> Option<&mut T> {
        None
    }

    #[inline]
    fn count_edit<T: Entry>(edits: &mut u32) {
        *edits = edits.wrapping_add(1);
    }

    #[inline]
    fn edits<T: Entry>(edits: &u32) -> u32 {
        *edits
    }
}

impl Layering for Layered {
    type Layer<T: Entry> = Option<Box<Layer<T>>>;
    type Below = Option<Arc<InternTable>>;
    type Own<T: Entry + Default> = Arc<T>;
    type Origin<T: Entry> = Option<Box<T>>;

    #[inline]
    fn layer<T: Entry>(layer: &Option<Box<Layer<T>>>) -> Option<&Layer<T>> {
        layer.as_deref()
    }

    #[inline]
    fn layer_mut<T: Entry>(layer: &mut Option<Box<Layer<T>>>) -> Option<&mut Layer<T>> {
        layer.as_deref_mut()
    }

    #[inline]
    fn below(below: &Option<Arc<InternTable>>) -> Option<&InternTable> {
        below.as_deref()
    }

    #[inline]
    fn own_mut<T: Entry + Default>(own: &mut Arc<T>) -> &mut T {
        Arc::make_mut(own)
    }

    #[inline]
    fn origin<T: Entry>(origin: &Option<Box<T>>) -> Option<&T> {
        origin.as_deref()
    }

    #[inline]
    fn origin_mut<T: Entry>(origin: &mut Option<Box<T>>) -> Option<&mut T> {
        origin.as_deref_mut()
    }

    #[inline]
    fn count_edit<T: Entry>(_: &mut Option<Box<T>>) {}

    #[inline]
    fn edits<T: Entry>(_: &Option<Box<T>>) -> u32 {
        0
    }
}

/// A table's vector (`spec/ir.md` §5.1, "Reading bodies in parallel").
/// In a module ([`NoLayer`]) it is a vector and nothing else. In a fork
/// ([`Layered`]) it is in two layers: a base shared by every fork, and
/// the entries added since, local to this one. Indices are global: entry
/// `i` is in the base below `off` and at `i - off` in the local part.
///
/// A tail ([`Seg::tail`]) has no base: it is the copy of a fork's entries
/// from `off` on, and reading below `off` is a bug.
#[derive(Clone, Debug)]
pub struct Seg<T: Entry, L: Layering = NoLayer> {
    local: Vec<T>,
    layer: L::Layer<T>,
}

/// A layer: where the base ends.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct Layer<T> {
    base: Option<Arc<Vec<T>>>,
    off: usize,
    /// Whether an entry of the base was changed ([`Seg::get_mut`]): a
    /// fork that did so cannot hand its work over (`Module::delta`).
    touched: bool,
}

impl<T: Entry, L: Layering> Default for Seg<T, L> {
    fn default() -> Self {
        Seg { local: Vec::new(), layer: L::Layer::<T>::default() }
    }
}

impl<T> Layer<T> {
    #[inline]
    fn base(&self) -> &[T] {
        match &self.base {
            Some(base) => base,
            None => unreachable!("an entry below a tail's start"),
        }
    }
}

impl<T: Entry, L: Layering> Seg<T, L> {
    #[inline]
    fn off(&self) -> usize {
        L::layer(&self.layer).map_or(0, |layer| layer.off)
    }

    /// Number of entries, both layers.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match L::layer(&self.layer) {
            None => self.local.len(),
            Some(layer) => layer.off + self.local.len(),
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Entry `i`, or `None` past the end.
    #[inline]
    pub(crate) fn get(&self, i: usize) -> Option<&T> {
        match L::layer(&self.layer) {
            None => self.local.get(i),
            Some(layer) => self.get_layered(layer, i),
        }
    }

    #[inline]
    fn get_layered<'a>(&'a self, layer: &'a Layer<T>, i: usize) -> Option<&'a T> {
        if i < layer.off { layer.base().get(i) } else { self.local.get(i - layer.off) }
    }

    /// `len` entries from `start`, which lie in one layer (a table's
    /// pooled parts are appended in one piece).
    #[inline]
    pub(crate) fn slice(&self, start: usize, len: usize) -> &[T] {
        match L::layer(&self.layer) {
            None => &self.local[start..][..len],
            Some(layer) => &self.tail_of_layer(layer, start)[..len],
        }
    }

    /// The entries from `start` to the end of the layer `start` is in.
    #[inline]
    pub(crate) fn from(&self, start: usize) -> &[T] {
        match L::layer(&self.layer) {
            None => &self.local[start..],
            Some(layer) => self.tail_of_layer(layer, start),
        }
    }

    #[inline]
    fn tail_of_layer<'a>(&'a self, layer: &'a Layer<T>, start: usize) -> &'a [T] {
        if start < layer.off {
            &layer.base()[start..layer.off]
        } else {
            &self.local[start - layer.off..]
        }
    }

    /// Every entry, base first.
    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + Clone + '_ {
        let base = L::layer(&self.layer).and_then(|layer| layer.base.as_deref());
        base.into_iter().flat_map(|base| base.iter()).chain(self.local.iter())
    }

    #[inline]
    pub(crate) fn push(&mut self, value: T) {
        self.local.push(value);
    }

    #[inline]
    pub(crate) fn extend_from_slice(&mut self, values: &[T]) {
        self.local.extend_from_slice(values);
    }

    pub(crate) fn append(&mut self, values: &mut Vec<T>) {
        self.local.append(values);
    }

    /// Drops the entries from `len` on, which lie in the local layer.
    pub(crate) fn truncate(&mut self, len: usize) {
        let off = self.off();
        debug_assert!(len >= off, "a truncation into the base");
        self.local.truncate(len - off);
    }

    /// Entry `i` to change in place. In a fork, an entry of the shared
    /// base is copied first (the whole base: the fork's work can then not
    /// be handed over).
    #[inline]
    pub(crate) fn get_mut(&mut self, i: usize) -> &mut T {
        if L::layer(&self.layer).is_none() {
            return &mut self.local[i];
        }
        self.get_mut_layered(i)
    }

    #[inline(never)]
    fn get_mut_layered(&mut self, i: usize) -> &mut T {
        let off = self.off();
        if i >= off {
            return &mut self.local[i - off];
        }
        let layer = L::layer_mut(&mut self.layer).expect("a layer below `off`");
        layer.touched = true;
        match &mut layer.base {
            Some(base) => &mut Arc::make_mut(base)[i],
            None => unreachable!("an entry below a tail's start"),
        }
    }

    /// Whether an entry below the local layer was changed since the
    /// table was forked.
    pub(crate) fn touched(&self) -> bool {
        L::layer(&self.layer).is_some_and(|layer| layer.touched)
    }

    /// The capacity of the local layer.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.local.capacity()
    }

    /// Whether the local layer is empty.
    pub(crate) fn local_is_empty(&self) -> bool {
        self.local.is_empty()
    }

    /// The bytes held, both layers (a size report).
    pub(crate) fn bytes(&self) -> usize {
        self.len() * size_of::<T>()
    }
}

impl<T: Entry> Seg<T, NoLayer> {
    /// The base of the forks of parallel reading: a copy of every entry,
    /// shared, and no entry of its own.
    pub(crate) fn seed(&self) -> Seg<T, Layered> {
        let base = Arc::new(self.local.clone());
        let off = self.local.len();
        Seg {
            local: Vec::new(),
            layer: Some(Box::new(Layer { base: Some(base), off, touched: false })),
        }
    }
}

impl<T: Entry> Seg<T, Layered> {
    /// A table with this one's base and no entry of its own. The local
    /// layer must be empty (a fork is made of a seed).
    pub(crate) fn fork(&self) -> Seg<T, Layered> {
        debug_assert!(self.local.is_empty(), "a fork of a table with local entries");
        let layer = self.layer.as_ref().map(|layer| {
            Box::new(Layer { base: layer.base.clone(), off: layer.off, touched: false })
        });
        Seg { local: Vec::new(), layer }
    }

    /// The entries from `from` on, copied, at their indices.
    pub(crate) fn tail(&self, from: usize) -> Seg<T, Layered> {
        let off = self.off();
        debug_assert!(from >= off, "a tail reaches into the base");
        let local = self.local[from - off..].to_vec();
        Seg { local, layer: Some(Box::new(Layer { base: None, off: from, touched: false })) }
    }
}

impl<T: Entry, L: Layering> std::ops::Index<usize> for Seg<T, L> {
    type Output = T;

    #[inline]
    fn index(&self, i: usize) -> &T {
        match L::layer(&self.layer) {
            None => &self.local[i],
            Some(layer) => self.get_layered(layer, i).expect("an index in the table"),
        }
    }
}

impl<T: Entry, L: Layering> std::ops::IndexMut<usize> for Seg<T, L> {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut T {
        self.get_mut(i)
    }
}

/// An open-addressing hash set of ids, keyed by caller-supplied hashes.
///
/// In a fork ([`Layered`]) the shared base comes first: a lookup probes
/// the base, then the local slots.
#[derive(Clone, Debug)]
pub struct InternTable<L: Layering = NoLayer> {
    base: L::Below,
    slots: Box<[Slot]>,
    len: usize,
}

impl<L: Layering> Default for InternTable<L> {
    fn default() -> Self {
        Self::new()
    }
}

impl InternTable<NoLayer> {
    /// The base of the forks of parallel reading: a copy, shared.
    pub(crate) fn seed(&self) -> InternTable<Layered> {
        InternTable { base: Some(Arc::new(self.clone())), ..InternTable::new() }
    }
}

impl InternTable<Layered> {
    /// A table with this one's base and no slot of its own.
    pub(crate) fn fork(&self) -> InternTable<Layered> {
        debug_assert_eq!(self.len, 0, "a fork of a table with local entries");
        InternTable { base: self.base.clone(), ..InternTable::new() }
    }
}

impl<L: Layering> InternTable<L> {
    const INITIAL: usize = 64;

    pub(crate) fn new() -> Self {
        Self {
            base: L::Below::default(),
            slots: vec![Slot { hash: 0, id: EMPTY }; Self::INITIAL].into(),
            len: 0,
        }
    }

    /// The id of an entry with `hash` for which `eq` holds.
    #[inline]
    pub(crate) fn find(&self, hash: u32, eq: impl FnMut(u32) -> bool) -> Option<u32> {
        match L::below(&self.base) {
            None => self.find_here(hash, eq),
            Some(base) => Self::find_layered(self, base, hash, eq),
        }
    }

    /// [`InternTable::find`] in a fork: the base, then the local slots.
    #[inline]
    fn find_layered(
        &self,
        base: &InternTable,
        hash: u32,
        mut eq: impl FnMut(u32) -> bool,
    ) -> Option<u32> {
        base.find_here(hash, &mut eq).or_else(|| self.find_here(hash, eq))
    }

    /// [`InternTable::find`] in the local slots alone.
    #[inline]
    fn find_here(&self, hash: u32, mut eq: impl FnMut(u32) -> bool) -> Option<u32> {
        let mask = self.slots.len() - 1;
        let mut at = hash as usize & mask;
        loop {
            let slot = self.slots[at];
            if slot.id == EMPTY {
                return None;
            }
            if slot.hash == hash && eq(slot.id) {
                return Some(slot.id);
            }
            at = (at + 1) & mask;
        }
    }

    /// Adds `id` under `hash`. The caller has checked with [`Self::find`]
    /// that no equal entry is present.
    pub(crate) fn insert(&mut self, hash: u32, id: u32) {
        debug_assert_ne!(id, EMPTY);
        // Grow at 3/4 load.
        if (self.len + 1) * 4 > self.slots.len() * 3 {
            let old = std::mem::take(&mut self.slots);
            self.slots = vec![Slot { hash: 0, id: EMPTY }; old.len() * 2].into();
            for slot in old {
                if slot.id != EMPTY {
                    self.place(slot);
                }
            }
        }
        self.place(Slot { hash, id });
        self.len += 1;
    }

    fn place(&mut self, slot: Slot) {
        let mask = self.slots.len() - 1;
        let mut at = slot.hash as usize & mask;
        while self.slots[at].id != EMPTY {
            at = (at + 1) & mask;
        }
        self.slots[at] = slot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §1: interning finds what was inserted, across growth and collisions.
    #[test]
    fn table_finds_inserted_ids_across_growth() {
        let mut table = <InternTable>::new();
        let keys: Vec<u64> = (0..5000).map(|i| i * 7919).collect();
        for (id, key) in keys.iter().enumerate() {
            // A deliberately poor hash (few distinct values) to force probing.
            let hash = (*key % 97) as u32;
            assert_eq!(table.find(hash, |other| keys[other as usize] == *key), None);
            table.insert(hash, id as u32);
        }
        for (id, key) in keys.iter().enumerate() {
            let hash = (*key % 97) as u32;
            assert_eq!(table.find(hash, |other| keys[other as usize] == *key), Some(id as u32));
        }
        assert_eq!(table.find(5, |_| false), None);
    }

    /// The hasher is a pure function of its input (principle 5: deterministic).
    #[test]
    fn hasher_is_deterministic_and_length_sensitive() {
        let hash = |bytes: &[u8]| {
            let mut h = Hasher64::new(1);
            h.bytes(bytes);
            h.finish()
        };
        assert_eq!(hash(b"abc"), hash(b"abc"));
        assert_ne!(hash(b"abc"), hash(b"abd"));
        assert_ne!(hash(b"abc\0"), hash(b"abc"));
        assert_ne!(hash(b"12345678"), hash(b"123456789"));
        let mut a = Hasher64::new(1);
        a.pair(1, 2);
        let mut b = Hasher64::new(1);
        b.pair(2, 1);
        assert_ne!(a.finish(), b.finish());
    }

    /// §1 `TableFull`: ids and pool positions stop at the `u32` limit.
    #[test]
    fn id_space_exhaustion_is_an_error() {
        assert_eq!(next_id(10, "type").unwrap(), 10);
        assert_eq!(next_id(MAX_ENTRIES, "type").unwrap_err().kind, IrErrorKind::TableFull);
        assert_eq!(pool_start(10, 5, "field").unwrap(), 10);
        assert_eq!(
            pool_start(MAX_ENTRIES - 1, 2, "field").unwrap_err().kind,
            IrErrorKind::TableFull
        );
        assert_eq!(pool_start(usize::MAX, 2, "field").unwrap_err().kind, IrErrorKind::TableFull);
    }
}
