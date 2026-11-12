// hod benchmark input: template-heavy C++ with many small functions.
//
// Written for this repository (MIT OR Apache-2.0). What it stands for:
// the object code of generic libraries: thousands of tiny `linkonce_odr`
// functions, each in its own COMDAT group, most of them a load, an
// operation and a return, called through several layers of equally small
// wrappers. Per-function overhead (symbols, sections, groups, unwind
// entries) dominates such a file, not instruction selection.

#include <cstddef>
#include <cstdint>
#include <utility>

namespace meta {

// A field of type T, distinguished by its index.
template <typename T, std::size_t I>
class Field {
public:
    constexpr Field() : value_(static_cast<T>(I)) {}
    const T &get() const { return value_; }
    void set(const T &value) { value_ = value; }
    std::uint64_t hash() const { return static_cast<std::uint64_t>(value_) * 1099511628211ull + I; }
    bool less(const Field &other) const { return value_ < other.value_; }
    template <typename F> void visit(F &&f) { f(I, value_); }

private:
    T value_;
};

// A record of N fields of T, built by inheritance from each field.
template <typename T, typename Indices> class Record;

template <typename T, std::size_t... I>
class Record<T, std::index_sequence<I...>> : public Field<T, I>... {
public:
    static constexpr std::size_t size = sizeof...(I);

    template <std::size_t K> const T &get() const { return Field<T, K>::get(); }
    template <std::size_t K> void set(const T &value) { Field<T, K>::set(value); }

    T sum() const { return (Field<T, I>::get() + ...); }
    std::uint64_t hash() const {
        std::uint64_t h = 14695981039346656037ull;
        ((h = (h ^ Field<T, I>::hash()) * 1099511628211ull), ...);
        return h;
    }
    bool less(const Record &other) const {
        bool result = false, decided = false;
        ((decided || (Field<T, I>::less(other)
                          ? (result = true, decided = true)
                          : static_cast<const Field<T, I> &>(other).less(*this) ? (decided = true) : false)),
         ...);
        return result;
    }
    template <typename F> void visit(F &&f) { (Field<T, I>::visit(f), ...); }
    void rotate() { (Field<T, I>::set(static_cast<T>(Field<T, I>::get() + static_cast<T>(I + 1))), ...); }
};

template <typename T, std::size_t N> using RecordOf = Record<T, std::make_index_sequence<N>>;

// Small function objects, one type per operation and operand type.
template <typename T> struct Add { T operator()(T a, T b) const { return static_cast<T>(a + b); } };
template <typename T> struct Mid { T operator()(T a, T b) const { return static_cast<T>((a + b) / 2); } };
template <typename T> struct Max { T operator()(T a, T b) const { return a < b ? b : a; } };

template <typename T, typename Op> struct Fold {
    T state;
    Op op;
    void operator()(std::size_t, const T &value) { state = op(state, value); }
};

template <typename T, std::size_t N, typename Op> T fold(RecordOf<T, N> &record, T initial) {
    Fold<T, Op> folder{initial, Op()};
    record.visit(folder);
    return folder.state;
}

// A compile-time list of types, and an operation applied to each.
template <typename... Ts> struct List {};

template <std::size_t N, typename T> std::uint64_t exercise() {
    RecordOf<T, N> a, b;
    b.rotate();
    std::uint64_t result = a.hash() ^ b.hash();
    result += static_cast<std::uint64_t>(fold<T, N, Add<T>>(a, T()));
    result += static_cast<std::uint64_t>(fold<T, N, Mid<T>>(b, static_cast<T>(1)));
    result += static_cast<std::uint64_t>(fold<T, N, Max<T>>(b, T()));
    result += a.less(b) ? 1 : 0;
    result += static_cast<std::uint64_t>(a.sum());
    return result;
}

template <std::size_t N, typename... Ts> std::uint64_t exercise_all(List<Ts...>) {
    return (exercise<N, Ts>() + ...);
}

} // namespace meta

int main() {
    using Types = meta::List<signed char, short, int, long, unsigned, unsigned long, float, double>;
    std::uint64_t total = meta::exercise_all<24>(Types()) + meta::exercise_all<48>(Types());
    meta::RecordOf<int, 48> record;
    record.set<7>(100);
    return total != 0 && record.get<7>() == 100 && record.get<8>() == 8 ? 0 : 1;
}
