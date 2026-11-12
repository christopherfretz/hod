// hod benchmark input: a typical C++ translation unit.
//
// Written for this repository (MIT OR Apache-2.0). What it stands for: an
// ordinary source file of an application: a handful of standard headers,
// a few classes of its own, containers of strings, smart pointers, an
// algorithm or two, and one place that throws.

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <map>
#include <memory>
#include <numeric>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace inventory {

enum class Unit { Piece, Gram, Litre };

struct Item {
    std::string name;
    Unit unit = Unit::Piece;
    std::int64_t quantity = 0;
    double price = 0.0;
    std::vector<std::string> tags;

    double value() const { return static_cast<double>(quantity) * price; }
    bool tagged(std::string_view tag) const {
        return std::find(tags.begin(), tags.end(), tag) != tags.end();
    }
};

class Store {
public:
    virtual ~Store() = default;
    virtual void put(Item item) = 0;
    virtual std::optional<Item> get(const std::string &name) const = 0;
    virtual std::vector<const Item *> all() const = 0;
};

class MapStore final : public Store {
public:
    void put(Item item) override {
        auto found = items_.find(item.name);
        if (found == items_.end()) {
            std::string key = item.name;
            items_.emplace(std::move(key), std::make_unique<Item>(std::move(item)));
        } else {
            found->second->quantity += item.quantity;
            found->second->price = item.price;
            for (auto &tag : item.tags) {
                if (!found->second->tagged(tag))
                    found->second->tags.push_back(std::move(tag));
            }
        }
    }

    std::optional<Item> get(const std::string &name) const override {
        auto found = items_.find(name);
        if (found == items_.end())
            return std::nullopt;
        return *found->second;
    }

    std::vector<const Item *> all() const override {
        std::vector<const Item *> out;
        out.reserve(items_.size());
        for (const auto &[name, item] : items_)
            out.push_back(item.get());
        return out;
    }

private:
    std::map<std::string, std::unique_ptr<Item>> items_;
};

struct ParseError : std::runtime_error {
    std::size_t line;
    ParseError(std::size_t line, const std::string &what)
        : std::runtime_error(what), line(line) {}
};

std::vector<std::string_view> split(std::string_view text, char separator) {
    std::vector<std::string_view> parts;
    std::size_t start = 0;
    while (start <= text.size()) {
        std::size_t end = text.find(separator, start);
        if (end == std::string_view::npos)
            end = text.size();
        parts.push_back(text.substr(start, end - start));
        start = end + 1;
    }
    return parts;
}

std::string_view trim(std::string_view text) {
    while (!text.empty() && (text.front() == ' ' || text.front() == '\t'))
        text.remove_prefix(1);
    while (!text.empty() && (text.back() == ' ' || text.back() == '\t' || text.back() == '\r'))
        text.remove_suffix(1);
    return text;
}

Unit parse_unit(std::string_view text, std::size_t line) {
    static const std::pair<std::string_view, Unit> units[] = {
        {"pc", Unit::Piece}, {"g", Unit::Gram}, {"l", Unit::Litre}};
    for (const auto &[name, unit] : units) {
        if (name == text)
            return unit;
    }
    throw ParseError(line, "unknown unit '" + std::string(text) + "'");
}

// One record per line: name, unit, quantity, price, tag tag ...
Item parse_item(std::string_view record, std::size_t line) {
    auto fields = split(record, ',');
    if (fields.size() < 4)
        throw ParseError(line, "expected at least 4 fields, found " + std::to_string(fields.size()));
    Item item;
    item.name = std::string(trim(fields[0]));
    if (item.name.empty())
        throw ParseError(line, "empty name");
    item.unit = parse_unit(trim(fields[1]), line);
    try {
        item.quantity = std::stoll(std::string(trim(fields[2])));
        item.price = std::stod(std::string(trim(fields[3])));
    } catch (const std::logic_error &error) {
        throw ParseError(line, std::string("bad number: ") + error.what());
    }
    if (fields.size() > 4) {
        for (auto tag : split(trim(fields[4]), ' ')) {
            if (!tag.empty())
                item.tags.emplace_back(tag);
        }
    }
    return item;
}

std::size_t load(Store &store, std::string_view text, std::vector<std::string> &errors) {
    std::size_t loaded = 0, number = 0;
    for (auto record : split(text, '\n')) {
        ++number;
        record = trim(record);
        if (record.empty() || record.front() == '#')
            continue;
        try {
            store.put(parse_item(record, number));
            ++loaded;
        } catch (const ParseError &error) {
            errors.push_back("line " + std::to_string(error.line) + ": " + error.what());
        }
    }
    return loaded;
}

struct Summary {
    double total = 0.0;
    std::string dearest;
    std::map<std::string, std::int64_t> by_tag;
    std::vector<std::string> names_by_value;
};

Summary summarize(const Store &store) {
    Summary summary;
    auto items = store.all();
    summary.total = std::accumulate(items.begin(), items.end(), 0.0,
                                    [](double sum, const Item *item) { return sum + item->value(); });
    std::sort(items.begin(), items.end(), [](const Item *a, const Item *b) {
        return a->value() != b->value() ? a->value() > b->value() : a->name < b->name;
    });
    if (!items.empty())
        summary.dearest = items.front()->name;
    for (const Item *item : items) {
        summary.names_by_value.push_back(item->name);
        for (const auto &tag : item->tags)
            summary.by_tag[tag] += item->quantity;
    }
    return summary;
}

std::string render(const Summary &summary) {
    std::string out;
    char number[64];
    std::snprintf(number, sizeof number, "%.2f", summary.total);
    out += "total " + std::string(number) + "\n";
    out += "dearest " + summary.dearest + "\n";
    for (const auto &[tag, quantity] : summary.by_tag)
        out += tag + "=" + std::to_string(quantity) + "\n";
    return out;
}

} // namespace inventory

int main() {
    static const char data[] =
        "# name, unit, quantity, price, tags\n"
        "bolt, pc, 400, 0.05, steel small\n"
        "flour, g, 5000, 0.002, food\n"
        "oil, l, 12, 3.5, food liquid\n"
        "bolt, pc, 100, 0.06, zinc\n"
        "broken, furlong, 1, 1\n"
        "short, pc\n";
    inventory::MapStore store;
    std::vector<std::string> errors;
    std::size_t loaded = inventory::load(store, data, errors);
    auto summary = inventory::summarize(store);
    std::string text = inventory::render(summary);
    bool ok = loaded == 4 && errors.size() == 2 && summary.dearest == "oil" &&
              summary.by_tag["food"] == 5012 && store.get("bolt")->quantity == 500 &&
              text.rfind("total 82.00\n", 0) == 0;
    return ok ? 0 : 1;
}
