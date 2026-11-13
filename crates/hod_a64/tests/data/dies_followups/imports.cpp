// Imported entities of every shape a front end emits (spec/dwarf.md §6.11):
// using-directives and using-declarations at namespace scope and in
// nested blocks, namespace aliases, using-declarations of a function, a
// variable and a type, and the standard library's own (#include, then
// `using namespace std;`).
#include <cstdlib>
#include <cmath>
#include <cstring>
#include <vector>
namespace lib {
int counter = 3;
int helper(int x);
struct Point { int x, y; };
namespace inner { int deep(int v); }
}
int lib::helper(int x) { return x + counter; }
int lib::inner::deep(int v) { return v * 2; }
using namespace std;
using lib::helper;
using lib::counter;
using lib::Point;
namespace al = lib::inner;
namespace other { using namespace lib; using lib::inner::deep; }
int user(int n) {
  using namespace lib;
  using lib::Point;
  Point p{n, n};
  if (n > 0) {
    using lib::counter;
    namespace a2 = lib;
    using lib::inner::deep;
    {
      using namespace lib::inner;
      p.x += deep(counter);
    }
  }
  vector<int> v{p.x, p.y};
  return helper(v[0]) + al::deep(v[1]);
}
int main() { return user(2) == 0; }
