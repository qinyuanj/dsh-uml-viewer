def mix(a, b):
    return ((a ^ b) & (a | b)) << ((a >> b) % b)
