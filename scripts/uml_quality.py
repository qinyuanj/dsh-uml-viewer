"""POSIX 质量分发：声明 scope、串行执行和原子结果记录。"""
import json
from pathlib import Path
import re
import subprocess
import sys

from uml_posix import atomic_write

TOOLS = {'crap': ('crapper', [0, 2]), 'mutate': ('mutator', [0, 3]), 'dry': ('dryer', [0])}
VALUES = {'--coverage-command', '--threshold', '--test-command', '--busted-task', '--timeout-factor', '--mutation-warning', '--max-workers', '--lines', '--min-lines', '--min-nodes', '--format'}
ROUTING = {'--root', '--source-root', '-s', '--exclude', '--full-tree', '--changed', '--mutate-all', '--edn', '--text'}


def strings(value):
    return isinstance(value, list) and all(isinstance(x, str) and x.strip() for x in value)


def read_config(root):
    path = root / 'uml-quality.json'
    if not path.exists():
        return {}
    config = json.loads(path.read_text())
    if not isinstance(config, dict) or type(config.get('version')) is not int or config['version'] != 1 or not {'standardTools', 'operations'} & config.keys() or 'aliasResultExitCodes' in config:
        raise ValueError('uml-quality.json 需要 version 1 和 standardTools 或 operations；Clojure alias 已移除。')
    if 'standardTools' in config:
        standard = config['standardTools']
        if not isinstance(standard, dict):
            raise ValueError('standardTools 必须是对象')
        for key, value in standard.items():
            if key not in {'sourceRoots', 'exclude', 'crapperOptions', 'mutatorOptions', 'dryerOptions'} or not strings(value):
                raise ValueError(f'无效 standardTools.{key}')
            if key.endswith('Options'):
                pending = False
                for option in value:
                    if pending:
                        pending = False
                    elif option in ROUTING or not option.startswith('--'):
                        raise ValueError(f'{key} 不允许 scope 或位置参数：{option}')
                    else:
                        pending = option in VALUES
                if pending:
                    raise ValueError(f'{key} 缺少参数值')
    if 'operations' in config:
        operations = config['operations']
        if not isinstance(operations, dict) or not {'crap', 'mutate'} <= operations.keys():
            raise ValueError('operations 必须声明 crap 和 mutate')
        for name in ('crap', 'mutate', 'dry'):
            if name not in operations:
                continue
            op = operations[name]
            if not isinstance(op, dict) or not isinstance(op.get('argv'), list) or not op['argv'] or not all(isinstance(x, str) for x in op['argv']) or not op['argv'][0].strip() or op.get('forwardArgs') not in ('append', 'none'):
                raise ValueError(f'无效 operations.{name}')
            codes = op.get('validResultExitCodes')
            if not isinstance(codes, list) or not codes or any(type(c) is not int or not 0 <= c <= 255 for c in codes):
                raise ValueError(f'{name} 需要 0–255 的 validResultExitCodes')
            if name == 'crap' and (op.get('scope') not in ('targets', 'whole-tree') or op['scope'] == 'whole-tree' and op['forwardArgs'] != 'none'):
                raise ValueError('whole-tree CRAP 必须 forwardArgs none；scope 仅接受 targets/whole-tree')
    return config


def scope(root, standard):
    roots = []
    for name in standard.get('sourceRoots', ['.']):
        if Path(name).is_absolute() or '..' in Path(name).parts:
            raise ValueError(f'业务源码根目录必须是项目内相对路径：{name}')
        path = (root / name).resolve()
        if not path.is_relative_to(root) or not path.is_dir():
            raise ValueError(f'无效业务源码根目录：{name}')
        roots.append(path)
    for name in standard.get('exclude', []):
        if Path(name).is_absolute() or '..' in Path(name).parts:
            raise ValueError(f'无效 exclude：{name}')
    return roots or [root]


def excluded(relative, patterns):
    for pattern in patterns:
        segments = pattern.removeprefix('./').rstrip('/').split('/')
        parts = []
        for index, segment in enumerate(segments):
            if segment == '**':
                parts.append('.*' if index == len(segments) - 1 else '(?:[^/]+/)*')
            else:
                parts.append(re.escape(segment).replace(r'\*', '[^/]*').replace(r'\?', '[^/]'))
                if index < len(segments) - 1:
                    parts.append('/')
        if re.match('^' + ''.join(parts) + '(?:/.*)?$', relative):
            return True
    return False


def business(root, path, roots, exclusions):
    canonical = path.resolve()
    return canonical.is_relative_to(root) and any(canonical.is_relative_to(p) for p in roots) and not excluded(path.relative_to(root).as_posix(), exclusions)


def scope_args(standard, flag):
    """声明的根目录和排除项，按工具自己的 -s/--exclude 选项传递。"""
    args = [value for source in standard.get('sourceRoots', []) for value in (flag, source)]
    return args + [value for pattern in standard.get('exclude', []) for value in ('--exclude', pattern)]


def split(forward):
    """分出调用方选项和目标；--changed 作为目标原样转发。"""
    options, names = [], []
    index = 0
    while index < len(forward):
        arg = forward[index]
        if arg in ('--root', '--source-root', '-s', '--exclude'):
            raise ValueError('mutation scope 由 standardTools 声明')
        if arg == '--changed' or not arg.startswith('-'):
            names.append(arg)
        else:
            options.append(arg)
            if arg in VALUES:
                index += 1
                if index == len(forward):
                    raise ValueError(f'{arg} 缺少参数值')
                options.append(forward[index])
        index += 1
    return options, names


def in_scope(root, path, roots, exclusions):
    """点名文件必须位于根目录内；点名目录也可以包含根目录。"""
    if business(root, path, roots, exclusions):
        return True
    holds = path.is_dir() and any(source.is_relative_to(path.resolve()) for source in roots)
    return holds and not excluded(path.relative_to(root).as_posix(), exclusions)


def target(root, name, roots, exclusions):
    """返回要转发的项目相对路径；scope 之外的点名目标返回 None。"""
    if name == '--changed':
        return name
    path = root / name
    if path.is_symlink():
        raise ValueError(f'mutation 不允许符号链接：{name}')
    if not path.exists():
        raise ValueError(f'mutation 目标不存在：{name}')
    if not path.resolve().is_relative_to(root):
        raise ValueError(f'mutation 目标位于项目之外：{name}')
    relative = path.relative_to(root).as_posix()
    if in_scope(root, path, roots, exclusions):
        return relative
    print(f'uml: omitted mutation target outside business scope: {relative}', file=sys.stderr)
    return None


def targets(root, forward, roots, exclusions):
    options, names = split(forward)
    if '--changed' in names and set(names) != {'--changed'}:
        # mutator 同时收到两者时只取变更集合，点名目标会被静默忽略。
        raise ValueError('--changed 不能与点名的 mutation 目标同时使用；请分开运行。')
    selection = []
    for name in names:
        chosen = target(root, name, roots, exclusions)
        if chosen and chosen not in selection:
            selection.append(chosen)
    if not selection:
        raise ValueError('没有业务 mutation 目标；请提供明确文件或 --changed。')
    return options, selection


def takes_scope(python, tool, root):
    """旧版 mutator/dryer 没有 --exclude，也不会把点名路径和 --changed 限制在 -s 之内。"""
    try:
        result = subprocess.run([str(python), '-m', tool, '--help'], cwd=root,
                                capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.SubprocessError):
        return False
    return result.returncode == 0 and '--exclude' in result.stdout


def standard_command(root, operation, standard, roots, forward):
    """标准工具的 argv 和 crapScope；声明的 scope 交给工具自己的 -s/--exclude。"""
    tool = TOOLS[operation][0]
    python = root / '.uml-viewer' / tool / '.venv/bin/python'
    if not python.is_file():
        raise ValueError(f'缺少 {python}；请重新运行 get-uml-viewer。')
    argv = [str(python), '-m', tool]
    options = standard.get(tool + 'Options', [])
    if operation == 'crap':
        return argv + ['--full-tree'] + scope_args(standard, '--source-root') + options + forward, 'whole-tree'
    extra, selection = targets(root, forward, roots, standard.get('exclude', [])) if operation == 'mutate' else ([], forward)
    scope = scope_args(standard, '-s')
    if scope and not takes_scope(python, tool, root):
        raise ValueError(f'.uml-viewer/{tool} 不接受 -s/--exclude 范围选择；请运行 get-uml-viewer --install-only 更新工具。')
    return argv + scope + options + extra + selection, None


def dispatch(root, operation, forward):
    result_path = root / '.uml-viewer/quality-result.json'
    result_path.unlink(missing_ok=True)
    config = read_config(root)
    standard = config.get('standardTools', {})
    roots = scope(root, standard)
    tool, codes = TOOLS[operation]
    override = config.get('operations', {}).get(operation)
    if override:
        argv = override['argv'] + (forward if override['forwardArgs'] == 'append' else [])
        if argv[0].lower().endswith(('.ps1', '.exe', '.bat', '.cmd')):
            raise ValueError('POSIX 质量命令需要原生可执行入口')
        commands = [argv]
        codes = override['validResultExitCodes']
        scope_name = override.get('scope') if operation == 'crap' else None
    else:
        if standard and any(x in ('--root', '--source-root', '-s', '--exclude') for x in forward):
            raise ValueError('业务 scope 已由 standardTools 声明，不能用 refresh 参数覆盖')
        argv, scope_name = standard_command(root, operation, standard, roots, forward)
        commands = [argv]
    valid = True
    code = 0
    for argv in commands:
        actual = subprocess.call(argv, cwd=root)
        actual = actual if actual >= 0 else 128 - actual
        valid = valid and actual in codes
        if actual or not code:
            code = actual
        if actual not in codes:
            break
    record = {'version': 1, 'operation': operation, 'projectRoot': str(root), 'exitCode': code,
              'validResult': valid, 'crapScope': scope_name, 'configured': bool(override),
              'tool': 'configured' if override else tool}
    try:
        atomic_write(result_path, json.dumps(record) + '\n')
    except OSError as error:
        print(f'uml: 无法发布质量结果记录：{error}', file=sys.stderr)
    return code
