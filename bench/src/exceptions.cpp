// hod benchmark input: exception-heavy C++.
//
// Written for this repository (MIT OR Apache-2.0). What it stands for:
// code where most calls are `invoke`s with cleanups: objects with
// destructors alive across calls that may throw, nested try blocks,
// catch clauses of several types, rethrows, function-try-blocks and
// noexcept boundaries. The unwind tables and call-site tables of such a
// file are a large part of its object code.

#include <exception>
#include <stdexcept>
#include <string>
#include <utility>

namespace eh {

int live_guards = 0;

struct Guard {
    const char *what;
    explicit Guard(const char *what) : what(what) { ++live_guards; }
    Guard(const Guard &other) : what(other.what) { ++live_guards; }
    ~Guard() { --live_guards; }
};

struct Error {
    int code;
    virtual ~Error() {}
    explicit Error(int code) : code(code) {}
    virtual const char *kind() const { return "error"; }
};
struct IoError : Error {
    using Error::Error;
    const char *kind() const override { return "io"; }
};
struct ParseError : Error {
    int column;
    ParseError(int code, int column) : Error(code), column(column) {}
    const char *kind() const override { return "parse"; }
};
struct Fatal {
    std::string message;
};

[[noreturn]] void fail(int which, int value) {
    switch (which % 6) {
    case 0: throw IoError(value);
    case 1: throw ParseError(value, value * 2);
    case 2: throw std::out_of_range("index " + std::to_string(value));
    case 3: throw std::invalid_argument("argument " + std::to_string(value));
    case 4: throw value;
    default: throw Fatal{"fatal " + std::to_string(value)};
    }
}

int may_fail(int which, int value) {
    Guard guard("may_fail");
    if (value % 7 == which % 7)
        fail(which, value);
    return value + which;
}

// A stage acquires three resources around three calls that may throw, and
// translates what it catches. One instantiation per N: the same shape of
// code many times over, as templates produce it.
template <int N>
struct Stage {
    static int run(int value) {
        Guard outer("outer");
        int result = 0;
        try {
            Guard first("first");
            result += may_fail(N, value);
            try {
                Guard second("second");
                result += may_fail(N + 1, value + 1);
                Guard third("third");
                result += may_fail(N + 2, value + 2);
            } catch (const ParseError &error) {
                result -= error.column;
                if (error.code % 2 == 0)
                    throw;
            } catch (const std::out_of_range &) {
                result -= 1;
            }
            result += may_fail(N + 3, value + 3);
        } catch (const IoError &error) {
            result = -error.code;
        } catch (const Error &error) {
            result = error.kind()[0];
        } catch (const std::exception &error) {
            result = static_cast<int>(std::string(error.what()).size());
        } catch (int thrown) {
            result = thrown * 1000;
        }
        return result;
    }

    static int checked(int value) noexcept {
        try {
            return run(value);
        } catch (const Fatal &fatal) {
            return -static_cast<int>(fatal.message.size());
        } catch (...) {
            return -9999;
        }
    }
};

// A constructor whose members may throw: a function-try-block, and the
// destruction of the members already built.
struct Pipeline {
    Guard a, b;
    std::string name;
    int value;

    static int checked_value(int value) {
        if (value < 0)
            throw std::invalid_argument("negative");
        return value;
    }

    Pipeline(int value, std::string name) try
        : a("a"), b("b"), name(std::move(name)), value(checked_value(value)) {
    } catch (const std::invalid_argument &) {
        // The exception is rethrown when the handler ends.
    }

    int run() const {
        int total = 0;
#define STAGE(n) total += Stage<n>::checked(value + n);
        STAGE(0) STAGE(1) STAGE(2) STAGE(3) STAGE(4) STAGE(5) STAGE(6) STAGE(7)
        STAGE(8) STAGE(9) STAGE(10) STAGE(11) STAGE(12) STAGE(13) STAGE(14) STAGE(15)
        STAGE(16) STAGE(17) STAGE(18) STAGE(19) STAGE(20) STAGE(21) STAGE(22) STAGE(23)
        STAGE(24) STAGE(25) STAGE(26) STAGE(27) STAGE(28) STAGE(29) STAGE(30) STAGE(31)
        STAGE(32) STAGE(33) STAGE(34) STAGE(35) STAGE(36) STAGE(37) STAGE(38) STAGE(39)
#undef STAGE
        return total;
    }
};

int nested_rethrow(int depth, int value) {
    Guard guard("nested");
    if (depth == 0)
        fail(value, value);
    try {
        return nested_rethrow(depth - 1, value) + 1;
    } catch (Error &error) {
        error.code += depth;
        throw;
    }
}

std::exception_ptr capture(int which) {
    try {
        fail(which, which);
    } catch (...) {
        return std::current_exception();
    }
}

} // namespace eh

int main() {
    int total = 0;
    for (int value = 0; value < 50; ++value) {
        eh::Pipeline pipeline(value, "p" + std::to_string(value));
        total += pipeline.run();
    }
    int rejected = 0;
    try {
        eh::Pipeline bad(-1, "bad");
    } catch (const std::invalid_argument &) {
        rejected = 1;
    }
    int deep = 0;
    try {
        eh::nested_rethrow(5, 6);
    } catch (const eh::Error &error) {
        deep = error.code;
    }
    int captured = 0;
    try {
        std::rethrow_exception(eh::capture(4));
    } catch (int value) {
        captured = value;
    }
    bool ok = eh::live_guards == 0 && rejected == 1 && deep == 6 + 15 && captured == 4 && total != 0;
    return ok ? 0 : 1;
}
