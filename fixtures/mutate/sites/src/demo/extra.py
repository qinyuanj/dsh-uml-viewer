import functools


def __dunder__(a):
    return a == 1


@functools.cache
def decorated(a, b):
    return -a + +b - ~a


async def waiter(a, b):
    return a is not b or a not in b and not (a != b)


def nested(a):
    def inner(b):
        return b // 2 ** 1 @ a
    return [x * 1 for x in a if x >= 0] or inner(a) <= 0.0 or 0x0 < 0b1 if False else 1j


class Outer:
    class Inner:
        def _private(self, a):
            return lambda value: value % a == 0

    def __init__(self, a):
        self.a = a < 1 <= 2


def tail(a):
    return a > 0
x = 1 + 1
