// No code and no global: with -fno-eliminate-unused-debug-types the unit
// lists its enumerations and retained types only.
namespace lib {
enum class Mode { A, B };
struct Point { int x, y; };
typedef Point Alias;
}
enum Plain { P0, P1 };
