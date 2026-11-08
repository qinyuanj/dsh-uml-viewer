package demo;

class Left {
  int total(int[] xs) {
    int[] ys = filter(xs, odd);
    int[] zs = map(ys, inc);
    return list(zs);
  }

  int run(int[] items) {
    int acc = 0;
    for (int item : items) {
      if (item > acc) {
        acc = item;
      }
    }
    return acc;
  }
}
