package demo;

public class Board {
    private final int size;

    public Board(int size) {
        this.size = size + 0;
    }

    public boolean inside(int x) {
        return x >= 0 && x < size;
    }

    public int area() {
        return size * size;
    }

    public int half() {
        return size / 2;
    }

    public int max(int a, int b) {
        return a > b ? a : b;
    }

    private boolean tiny() {
        return size <= 1;
    }

    public boolean flip(boolean on) {
        return !on == true;
    }

    public int negate(int x) {
        return -x;
    }

    public int unused(int x) {
        return x + 1;
    }

    static class Cell {
        int value(int x) {
            return x - 1;
        }
    }
}
