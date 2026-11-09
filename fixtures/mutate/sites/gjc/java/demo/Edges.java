package demo;

import java.util.function.IntUnaryOperator;

public class Edges {
    @Override
    public String toString() { return "a + 1" + 0; }

    public int ops(int a, int b, boolean ok) {
        int x = -a + +b - 1;
        x += 1;
        x++;
        long y = 0L + 1L + 0x1 + 01 + 1_0;
        double d = 1.0 + 0.0;
        char c = '1';
        if (!ok || a != b || a <= b || a >= b || a < 0 || a == 1) {
            x = ~x;
        }
        boolean t = true, f = false;
        return x > 0 ? a % b : (int) y >>> 1;
    }

    private static int secret() { return 0; }

    protected int prot() { return 1; }

    /** private in a comment */
    int javadoc() { return 0; }

    public Edges() { int z = 1 + 0; }

    IntUnaryOperator lambda() {
        return v -> v + 1;
    }

    Object anon() {
        return new Object() {
            int inner() { return 1 - 0; }
        };
    }

    static class Nested {
        private void deep() { boolean q = !true; }
    }

    enum Kind {
        A, B;
        int code() { return this == A ? 1 : 0; }
    }

    interface Shape {
        default int sides() { return 0 + 1; }
    }

    record Point(int x, int y) {
        int sum() { return x + y; }
    }
}
