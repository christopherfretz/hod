// Calls the functions of hand.ll (spec/dwarf.md 6.4, 8.7), whose DIEs
// the debugger sessions read: split's block has two ranges.
int split(int v);
int declared_only(int v);
int with_label(int v, int unused);
void no_vars(void);

int main(void) {
  int r = split(5) + declared_only(1) + with_label(0, 9);
  no_vars();
  return r != 9;
}
