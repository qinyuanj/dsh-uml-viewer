package demo;

class LegacyTest {
  int total(int[] xs) {
    int[] ys = filter(xs, odd);
    int[] zs = map(ys, inc);
    return list(zs);
  }
}
