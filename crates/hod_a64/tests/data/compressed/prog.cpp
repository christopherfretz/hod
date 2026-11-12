// Compressed debug sections (spec/object.md "Compressed sections",
// WP-061): a program whose DIEs, line table and strings the debuggers
// read through hod's zlib streams.
namespace shapes {

struct Rect {
  int width;
  int height;
  int area() const;
};

int Rect::area() const {
  int a = width * height;
  return a;
}

enum class Colour { Red, Green, Blue };

int weight(Colour c, int base) {
  int w = base;
  if (c == Colour::Green)
    w += 10;
  return w;
}

} // namespace shapes

static int total = 0;

int accumulate(const shapes::Rect *rects, int n) {
  int sum = 0;
  for (int i = 0; i < n; ++i) {
    int a = rects[i].area();
    sum += a;
  }
  total += sum;
  return sum;
}

int main() {
  shapes::Rect rects[3] = {{2, 3}, {4, 5}, {6, 7}};
  int s = accumulate(rects, 3);
  int w = shapes::weight(shapes::Colour::Green, s);
  return w == 78 && total == 68 ? 0 : 1;
}
