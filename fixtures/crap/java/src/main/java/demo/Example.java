package demo;

public class Example {
    public Example() {
        if (System.nanoTime() == 0) {
            throw new IllegalStateException("constructor is not a scored method");
        }
    }

    // The local method is below a lambda, so it is not a scored entry.
    private final Runnable local = () -> {
        class Local {
            int ignored() {
                if (true) return 1;
                return 0;
            }
        }
        new Local().ignored();
    };

    public int score(boolean a, boolean b, int[] values) {
        for (int i = 0; i < values.length; i++) {
        }
        for (int value : values) {
        }
        while (a) {
            a = false;
        }
        do {
            b = false;
        } while (b);
        if (a && b || values.length > 0) {
        }
        try {
            return a ? 1 : 0;
        } catch (RuntimeException ex) {
            return 2;
        }
    }

    public int nested(boolean a, boolean b, int[] values) {
        for (int i = 0; i < values.length; i++) {
            if (a) {
            }
        }
        for (int value : values) {
            if (b) {
            }
        }
        while (a) {
            if (b) {
            }
            a = false;
        }
        do {
            if (a) {
            }
            b = false;
        } while (b);
        try {
            return a ? (b ? 1 : 0) : 2;
        } catch (RuntimeException ex) {
            if (values.length > 0) {
                return values[0];
            }
            return 3;
        }
    }

    public int switched(int value) {
        switch (value) {
            case 1:
                if (value > 0) {
                    return 1;
                }
                return 0;
            default:
                return 2;
        }
    }

    public int outer() {
        Runnable runnable = new Runnable() {
            @Override
            public void run() {
                if (true) {
                }
            }
        };
        runnable.run();
        local.run();
        return 1;
    }

    public int stable() {
        String text = "if && || ? case default catch";
        // if && || ? case default catch
        /* if && || ? case default catch */
        return text.length();
    }

    public int overloaded(int value) {
        if (value > 0) return value;
        return 0;
    }

    public int overloaded(boolean value) {
        return value ? 1 : 0;
    }

    public int uncovered(boolean value) {
        if (value) return 1;
        return 0;
    }

    public static class Inner {
        public int tick() {
            return 1;
        }
    }

    public interface Contract {
        int absent();

        default int present(boolean value) {
            return value ? 1 : 0;
        }
    }

    public enum Mode {
        READY;

        public int code() {
            return 1;
        }
    }

    public record Value(int value) {
        public int positive() {
            return value > 0 ? value : 0;
        }
    }
}
