#!/usr/bin/env python3
import sys
from pathlib import Path


class Reader:
    def __init__(self, text):
        self.text = text
        self.at = 0

    def error(self, message):
        raise ValueError(f'{message} at byte {self.at}')

    def skip(self):
        while self.at < len(self.text):
            c = self.text[self.at]
            if c in ' \t\r\n,':
                self.at += 1
            elif c == ';':
                while self.at < len(self.text) and self.text[self.at] != '\n':
                    self.at += 1
            else:
                return

    def read(self):
        self.skip()
        if self.at >= len(self.text):
            self.error('unexpected end of input')
        if self.text.startswith('#_', self.at):
            self.at += 2
            self.read()
            return self.read()
        c = self.text[self.at]
        if c == '{':
            items = self.collection('}')
            if len(items) % 2:
                self.error('map has an odd number of forms')
            return ('map', [(items[i], items[i + 1]) for i in range(0, len(items), 2)])
        if c == '[':
            return ('vector', self.collection(']'))
        if c == '(':
            return ('list', self.collection(')'))
        if c == '#':
            if self.text.startswith('#{', self.at):
                self.at += 1
                return ('set', self.collection('}'))
            self.error('unsupported dispatch')
        if c == '"':
            return ('string', self.string())
        if c in ':':
            return ('keyword', self.token())
        if c in ')]}':
            self.error(f'unexpected {c}')
        token = self.token()
        if token == 'nil':
            return ('nil', None)
        if token == 'true':
            return ('bool', True)
        if token == 'false':
            return ('bool', False)
        try:
            return ('number', int(token))
        except ValueError:
            try:
                return ('number', float(token))
            except ValueError:
                return ('symbol', token)

    def collection(self, close):
        self.at += 1
        items = []
        while True:
            self.skip()
            if self.at >= len(self.text):
                self.error(f'missing {close}')
            if self.text[self.at] == close:
                self.at += 1
                return items
            items.append(self.read())

    def token(self):
        start = self.at
        while self.at < len(self.text) and self.text[self.at] not in ' \t\r\n,[]{}()";':
            self.at += 1
        if start == self.at:
            self.error('empty token')
        return self.text[start:self.at]

    def string(self):
        self.at += 1
        out = []
        while self.at < len(self.text):
            c = self.text[self.at]
            self.at += 1
            if c == '"':
                return ''.join(out)
            if c != '\\':
                out.append(c)
                continue
            if self.at >= len(self.text):
                self.error('unterminated escape')
            escape = self.text[self.at]
            self.at += 1
            simple = {'n': '\n', 'r': '\r', 't': '\t', 'b': '\b', 'f': '\f',
                      '"': '"', '\\': '\\', '/': '/'}
            if escape in simple:
                out.append(simple[escape])
            elif escape == 'u':
                digits = self.text[self.at:self.at + 4]
                if len(digits) != 4:
                    self.error('short unicode escape')
                out.append(chr(int(digits, 16)))
                self.at += 4
            else:
                out.append(escape)
        self.error('unterminated string')


def parse(text):
    reader = Reader(text)
    value = reader.read()
    reader.skip()
    if reader.at != len(reader.text):
        reader.error('trailing input')
    return value


def canonical(value):
    kind, body = value
    if kind == 'map':
        pairs = sorted(((canonical(k), canonical(v)) for k, v in body), key=repr)
        return (kind, tuple(pairs))
    if kind in ('vector', 'list', 'set'):
        items = [canonical(v) for v in body]
        if kind == 'set':
            items.sort()
        return (kind, tuple(items))
    return value


def compare(left, right, path='$'):
    if left[0] != right[0]:
        raise AssertionError(f'{path}: {left[0]} vs {right[0]}')
    kind = left[0]
    if kind in ('map',):
        a, b = dict(left[1]), dict(right[1])
        for key in sorted(set(a) | set(b), key=repr):
            if key not in a:
                raise AssertionError(f'{path}: extra key {key[1]}')
            if key not in b:
                raise AssertionError(f'{path}: missing key {key[1]}')
            compare(a[key], b[key], f'{path}.{key[1]}')
    elif kind in ('vector', 'list', 'set'):
        a, b = list(left[1]), list(right[1])
        if kind == 'set':
            a, b = sorted(a), sorted(b)
        if len(a) != len(b):
            raise AssertionError(f'{path}: length {len(a)} vs {len(b)}')
        for index, (x, y) in enumerate(zip(a, b)):
            compare(x, y, f'{path}[{index}]')
    elif kind == 'number':
        if left[1] != right[1]:
            raise AssertionError(f'{path}: {left[1]} vs {right[1]}')
    elif left != right:
        raise AssertionError(f'{path}: {left[1]!r} vs {right[1]!r}')


def main(argv):
    if len(argv) != 3:
        print('usage: compare_ir.py <actual.edn> <golden.edn>', file=sys.stderr)
        return 2
    actual = canonical(parse(Path(argv[1]).read_text()))
    golden = canonical(parse(Path(argv[2]).read_text()))
    try:
        compare(actual, golden)
    except AssertionError as error:
        print(f'IR semantic comparison failed: {argv[1]} vs {argv[2]}: {error}', file=sys.stderr)
        return 1
    print('IR semantic comparison passed')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
