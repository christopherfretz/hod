// Debug-information survey input: a translation unit that includes the
// standard library, for sizes (spec/ir.md §6.10; tests/debug_survey.rs).
#include <algorithm>
#include <iostream>
#include <map>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>
#include <functional>
#include <sstream>
struct Item { std::string name; std::vector<int> values; std::shared_ptr<Item> next; };
int main(int argc, char **argv) {
  std::map<std::string, Item> items;
  std::unordered_map<int, std::unique_ptr<Item>> by_id;
  for (int i = 0; i < argc; ++i) {
    Item it{argv[i], {i, i + 1}, nullptr};
    std::sort(it.values.begin(), it.values.end(), std::greater<int>());
    items.emplace(it.name, it);
    by_id[i] = std::make_unique<Item>(it);
  }
  std::ostringstream os;
  std::function<void(const Item &)> show = [&](const Item &it) { os << it.name << ' ' << it.values.size() << '\n'; };
  for (auto &[k, v] : items) show(v);
  std::cout << os.str();
  return (int)by_id.size();
}
