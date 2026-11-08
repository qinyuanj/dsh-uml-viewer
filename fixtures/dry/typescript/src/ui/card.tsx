export const Card = (items: number[]): number[] => {
  const rows = items.map((item: number) => item + 1);
  return rows.filter(Boolean);
};
