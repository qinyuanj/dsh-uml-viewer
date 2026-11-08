
class Sample {
    int outer() {
        Runnable runnable = new Runnable() {
            @Override
            public void run() {
                if (true) {
                }
            }
        };
        return 1;
    }
}
