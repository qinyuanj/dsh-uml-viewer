package demo;

abstract class Base {
  abstract int declared(int[] xs);
}

class Right {
  int sum(int[] xs) {
    int[] ys = filter(xs, odd);
    int[] zs = map(ys, inc);
    return list(zs);
  }

  int walk(int[] items) {
    int top = 0;
    for (int value : items) {
      if (value > top) {
        top = value;
      }
    }
    return top;
  }
}
