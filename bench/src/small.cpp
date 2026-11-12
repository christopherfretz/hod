// hod benchmark input: a small C++ translation unit without library
// headers.
//
// Written for this repository (MIT OR Apache-2.0). What it stands for: a
// small class hierarchy with virtual calls, one template and one
// exception, with no standard-library code behind it.

namespace shapes {

struct Shape {
    virtual ~Shape() {}
    virtual long area() const = 0;
    virtual const char *name() const = 0;
};

struct Rect final : Shape {
    long w, h;
    Rect(long w, long h) : w(w), h(h) {}
    long area() const override { return w * h; }
    const char *name() const override { return "rect"; }
};

struct Square final : Shape {
    long side;
    explicit Square(long side) : side(side) {}
    long area() const override { return side * side; }
    const char *name() const override { return "square"; }
};

template <typename T, int N>
struct Ring {
    T items[N] = {};
    int head = 0;
    int size = 0;

    void push(T value) {
        items[(head + size) % N] = value;
        if (size < N)
            ++size;
        else
            head = (head + 1) % N;
    }

    T sum() const {
        T total = T();
        for (int i = 0; i < size; ++i)
            total += items[(head + i) % N];
        return total;
    }
};

struct Empty {};

long total_area(const Shape *const *shapes, int count) {
    if (count == 0)
        throw Empty();
    Ring<long, 4> last;
    for (int i = 0; i < count; ++i)
        last.push(shapes[i]->area());
    return last.sum();
}

} // namespace shapes

int main() {
    shapes::Rect r(3, 5);
    shapes::Square s(4);
    const shapes::Shape *all[] = {&r, &s, &r};
    long total = 0;
    try {
        total = shapes::total_area(all, 3) + shapes::total_area(all, 0);
    } catch (const shapes::Empty &) {
        total = -1;
    }
    return total == -1 && all[1]->name()[0] == 's' ? 0 : 1;
}
