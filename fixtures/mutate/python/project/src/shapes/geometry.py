"""Shapes the tests measure."""


class Board:
    def __init__(self, width: int, height: int | None = None) -> None:
        self.width = width
        self.height = width if height is None else height

    def area(self) -> int:
        return self.width * self.height

    def contains(self, x: int, y: int) -> bool:
        return 0 <= x < self.width and 0 <= y < self.height

    def _edge(self, x: int) -> bool:
        return x == 0 or x == self.width - 1


def scale(value: int, factor: int | None) -> int:
    if factor is None:
        return value
    return value * factor


def flags(a: int, b: int) -> int:
    return (a ^ b) | (a & b) << 1


def parity(n: int) -> int:
    return n % 2


def is_member(item: str, items: list[str]) -> bool:
    return item not in items


def negate(flag: bool) -> bool:
    if not flag:
        return True
    return False


def untested(x: int) -> int:
    label = "雪 > 0 and True"
    return -x + len(label) // 1
