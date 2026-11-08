export function Badge(props: { count: number }) {
  return props.count > 0 ? <span>{props.count}</span> : null;
}
