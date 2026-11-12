// A translation unit heavy in the standard library (spec/dwarf.md §7.2,
// §8.6): the type graph of everything its functions reach, measured and
// compared with `llc`.
#include <functional>
#include <iostream>
#include <map>
#include <memory>
#include <string>
#include <vector>

struct Shape {
  virtual ~Shape() = default;
  virtual double area() const = 0;
};
struct Square : Shape {
  explicit Square(double s) : side(s) {}
  double area() const override { return side * side; }
  double side;
};

std::vector<int> numbers;
std::map<std::string, std::vector<double>> table;
std::string name = "hod";
std::unique_ptr<Shape> shape;
std::shared_ptr<std::string> shared;
std::function<int(int)> callback;

int run(const std::string &key) {
  std::vector<std::unique_ptr<Shape>> shapes;
  shapes.push_back(std::make_unique<Square>(2.0));
  table[key].push_back(shapes.front()->area());
  auto copy = std::make_shared<std::string>(key);
  std::function<int(int)> twice = [](int x) { return 2 * x; };
  for (auto &[k, v] : table)
    std::cout << k << ' ' << v.size() << '\n';
  numbers.push_back(twice(static_cast<int>(copy->size())));
  return numbers.back();
}

int main() { return run(name); }
