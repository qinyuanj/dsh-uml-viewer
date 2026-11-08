def mix(a, b):
    s = "^ & | << >> %"
    # ^ & | << >> %
    a ^= b
    a &= b
    a |= b
    a <<= b
    a >>= b
    a %= b
