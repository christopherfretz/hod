// A translation unit that instantiates a good part of the standard
// library (spec/ll.md §9, §10): several megabytes of IR at -O0.
#include <algorithm>
#include <functional>
#include <iostream>
#include <map>
#include <memory>
#include <regex>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

struct Base {
  virtual ~Base();
  virtual int f() const = 0;
};
struct Derived : Base {
  int f() const override { return 42; }
};
thread_local int tl = 5;
static std::map<std::string, std::vector<int>> table;

int run(const std::string &s) {
  std::unordered_map<std::string, std::function<int(int)>> fns;
  fns["a"] = [](int x) { return x + 1; };
  std::regex re("a+b*");
  std::ostringstream os;
  os << s << std::regex_match(s, re);
  std::unique_ptr<Base> p = std::make_unique<Derived>();
  table[s].push_back(p->f());
  std::sort(table[s].begin(), table[s].end());
  std::cout << os.str() << fns["a"](3) << tl;
  return (int)table.size();
}
