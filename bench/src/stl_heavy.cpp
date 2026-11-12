// hod benchmark input: a large, library-heavy C++ translation unit.
//
// Written for this repository (MIT OR Apache-2.0). What it stands for:
// the expensive files of a C++ build: a dozen standard headers, streams,
// regular expressions, associative containers of several key and value
// types, variants and visitors, type-erased callables, shared ownership.
// Nearly all of the resulting functions are instantiated library
// templates that this file did not write.

#include <algorithm>
#include <array>
#include <cstdint>
#include <deque>
#include <functional>
#include <iomanip>
#include <iostream>
#include <map>
#include <memory>
#include <numeric>
#include <optional>
#include <regex>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <tuple>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <variant>
#include <vector>

namespace logs {

enum class Level { Debug, Info, Warning, Error };

using Value = std::variant<std::monostate, bool, std::int64_t, double, std::string, std::vector<std::string>>;

struct Event {
    std::uint64_t time = 0;
    Level level = Level::Info;
    std::string source;
    std::string message;
    std::map<std::string, Value> fields;
};

struct Printer {
    std::ostream &out;
    void operator()(std::monostate) const { out << "null"; }
    void operator()(bool value) const { out << (value ? "true" : "false"); }
    void operator()(std::int64_t value) const { out << value; }
    void operator()(double value) const { out << std::fixed << std::setprecision(3) << value; }
    void operator()(const std::string &value) const { out << std::quoted(value); }
    void operator()(const std::vector<std::string> &values) const {
        out << '[';
        for (std::size_t i = 0; i < values.size(); ++i)
            out << (i ? "," : "") << std::quoted(values[i]);
        out << ']';
    }
};

std::ostream &operator<<(std::ostream &out, const Value &value) {
    std::visit(Printer{out}, value);
    return out;
}

std::ostream &operator<<(std::ostream &out, Level level) {
    static const std::array<std::string_view, 4> names = {"debug", "info", "warning", "error"};
    return out << names[static_cast<std::size_t>(level)];
}

std::optional<Level> parse_level(const std::string &text) {
    static const std::unordered_map<std::string, Level> levels = {
        {"debug", Level::Debug}, {"info", Level::Info}, {"warn", Level::Warning},
        {"warning", Level::Warning}, {"error", Level::Error}};
    auto found = levels.find(text);
    if (found == levels.end())
        return std::nullopt;
    return found->second;
}

Value parse_value(const std::string &text) {
    static const std::regex integer(R"([-+]?[0-9]+)");
    static const std::regex real(R"([-+]?[0-9]*\.[0-9]+([eE][-+]?[0-9]+)?)");
    if (text.empty() || text == "null")
        return std::monostate();
    if (text == "true" || text == "false")
        return text == "true";
    if (std::regex_match(text, integer))
        return static_cast<std::int64_t>(std::stoll(text));
    if (std::regex_match(text, real))
        return std::stod(text);
    if (text.find('|') != std::string::npos) {
        std::vector<std::string> parts;
        std::istringstream stream(text);
        for (std::string part; std::getline(stream, part, '|');)
            parts.push_back(part);
        return parts;
    }
    return text;
}

// "<time> <level> <source>: <message> key=value key=value"
std::optional<Event> parse_event(const std::string &line) {
    static const std::regex shape(R"(^(\d+)\s+(\w+)\s+([\w./-]+):\s*(.*)$)");
    static const std::regex field(R"((\w+)=(\S+))");
    std::smatch match;
    if (!std::regex_match(line, match, shape))
        return std::nullopt;
    auto level = parse_level(match[2].str());
    if (!level)
        return std::nullopt;
    Event event;
    event.time = std::stoull(match[1].str());
    event.level = *level;
    event.source = match[3].str();
    std::string rest = match[4].str();
    auto begin = std::sregex_iterator(rest.begin(), rest.end(), field);
    std::size_t message_end = rest.size();
    for (auto it = begin; it != std::sregex_iterator(); ++it) {
        message_end = std::min(message_end, static_cast<std::size_t>(it->position(0)));
        event.fields[(*it)[1].str()] = parse_value((*it)[2].str());
    }
    event.message = rest.substr(0, message_end);
    while (!event.message.empty() && event.message.back() == ' ')
        event.message.pop_back();
    return event;
}

class Sink {
public:
    virtual ~Sink() = default;
    virtual void accept(const std::shared_ptr<const Event> &event) = 0;
    virtual void report(std::ostream &out) const = 0;
};

class CountBySource final : public Sink {
public:
    void accept(const std::shared_ptr<const Event> &event) override { ++counts_[event->source][event->level]; }
    void report(std::ostream &out) const override {
        std::vector<std::pair<std::string, std::size_t>> totals;
        for (const auto &[source, by_level] : counts_) {
            std::size_t total = std::accumulate(
                by_level.begin(), by_level.end(), std::size_t{0},
                [](std::size_t sum, const auto &entry) { return sum + entry.second; });
            totals.emplace_back(source, total);
        }
        std::sort(totals.begin(), totals.end(), [](const auto &a, const auto &b) {
            return std::tie(b.second, a.first) < std::tie(a.second, b.first);
        });
        for (const auto &[source, total] : totals)
            out << std::setw(12) << std::left << source << ' ' << total << '\n';
    }

private:
    std::unordered_map<std::string, std::map<Level, std::size_t>> counts_;
};

class Recent final : public Sink {
public:
    explicit Recent(std::size_t limit) : limit_(limit) {}
    void accept(const std::shared_ptr<const Event> &event) override {
        if (event->level < Level::Warning)
            return;
        recent_.push_back(event);
        if (recent_.size() > limit_)
            recent_.pop_front();
    }
    void report(std::ostream &out) const override {
        for (const auto &event : recent_) {
            out << event->time << ' ' << event->level << ' ' << event->source << ": " << event->message;
            for (const auto &[key, value] : event->fields)
                out << ' ' << key << '=' << value;
            out << '\n';
        }
    }

private:
    std::size_t limit_;
    std::deque<std::shared_ptr<const Event>> recent_;
};

class FieldStats final : public Sink {
public:
    void accept(const std::shared_ptr<const Event> &event) override {
        for (const auto &[key, value] : event->fields) {
            keys_.insert(key);
            if (const auto *number = std::get_if<std::int64_t>(&value))
                sums_[key] += static_cast<double>(*number);
            else if (const auto *real = std::get_if<double>(&value))
                sums_[key] += *real;
            else if (const auto *list = std::get_if<std::vector<std::string>>(&value))
                for (const auto &item : *list)
                    seen_[key].insert(item);
        }
    }
    void report(std::ostream &out) const override {
        for (const auto &key : keys_) {
            out << key;
            if (auto sum = sums_.find(key); sum != sums_.end())
                out << " sum=" << std::setprecision(6) << std::defaultfloat << sum->second;
            if (auto seen = seen_.find(key); seen != seen_.end())
                out << " distinct=" << seen->second.size();
            out << '\n';
        }
    }

private:
    std::set<std::string> keys_;
    std::map<std::string, double> sums_;
    std::unordered_map<std::string, std::unordered_set<std::string>> seen_;
};

class Pipeline {
public:
    using Filter = std::function<bool(const Event &)>;

    void add_filter(Filter filter) { filters_.push_back(std::move(filter)); }
    void add_sink(std::unique_ptr<Sink> sink) { sinks_.push_back(std::move(sink)); }

    std::tuple<std::size_t, std::size_t, std::size_t> feed(std::istream &in) {
        std::size_t lines = 0, parsed = 0, kept = 0;
        for (std::string line; std::getline(in, line);) {
            ++lines;
            auto event = parse_event(line);
            if (!event)
                continue;
            ++parsed;
            if (!std::all_of(filters_.begin(), filters_.end(), [&](const Filter &f) { return f(*event); }))
                continue;
            ++kept;
            auto shared = std::make_shared<const Event>(std::move(*event));
            for (auto &sink : sinks_)
                sink->accept(shared);
        }
        return {lines, parsed, kept};
    }

    std::string report() const {
        std::ostringstream out;
        for (const auto &sink : sinks_) {
            sink->report(out);
            out << "--\n";
        }
        return out.str();
    }

private:
    std::vector<Filter> filters_;
    std::vector<std::unique_ptr<Sink>> sinks_;
};

} // namespace logs

int main() {
    std::istringstream input(
        "100 info net/http: request served status=200 ms=12.5 tags=a|b\n"
        "101 debug net/http: headers parsed count=14\n"
        "102 warn disk: space low free=0.071 mount=/var\n"
        "garbage line\n"
        "103 error net/http: upstream failed status=502 retry=true tags=b|c\n"
        "104 loud disk: unknown level\n"
        "105 info auth: login user=ada ok=true\n");
    logs::Pipeline pipeline;
    pipeline.add_filter([](const logs::Event &event) { return event.level != logs::Level::Debug; });
    pipeline.add_filter([minimum = std::uint64_t{100}](const logs::Event &event) { return event.time >= minimum; });
    pipeline.add_sink(std::make_unique<logs::CountBySource>());
    pipeline.add_sink(std::make_unique<logs::Recent>(8));
    pipeline.add_sink(std::make_unique<logs::FieldStats>());
    auto [lines, parsed, kept] = pipeline.feed(input);
    std::string report = pipeline.report();
    bool ok = lines == 7 && parsed == 5 && kept == 4 &&
              report.find("net/http     2") != std::string::npos &&
              report.find("status sum=702") != std::string::npos &&
              report.find("tags distinct=3") != std::string::npos &&
              report.find("mount=\"/var\"") != std::string::npos;
    if (!ok)
        std::cerr << report;
    return ok ? 0 : 1;
}
