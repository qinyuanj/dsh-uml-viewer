class Mix { int mix(int a, int b) { return ((a ^ b) & (a | b)) << ((a >> b) % b); } }
