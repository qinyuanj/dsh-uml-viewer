export function Panel(props: { open: boolean; count: number }) {
  return props.open && <div hidden={!props.open}>{props.count + 1 > 0 ? "yes" : null}</div>;
}

export const Item = ({ value }: { value: number }) => <li>{value * 2}</li>;
