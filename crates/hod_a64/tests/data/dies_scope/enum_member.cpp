#include <cstdint>
enum class E : uint8_t { A = 1, B = 200 };
struct S { static constexpr E e = E::B; static const E f = E::B; };
int use() { return (int)S::e + (int)S::f; }
int main() { return use(); }
