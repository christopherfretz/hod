// A translation unit without code or globals: only what it lists.
#include <cstdlib>
#include <cmath>
#include <vector>
namespace lib {
int helper(int);
extern int counter;
struct Point { int x, y; };
enum class Mode { A, B };
}
using namespace std;
using lib::helper;
using lib::counter;
using lib::Point;
namespace al = lib;
