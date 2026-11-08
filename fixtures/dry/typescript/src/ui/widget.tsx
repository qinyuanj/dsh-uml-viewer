export const Widget = (xs: number[]): number[] => {
  const rows = xs.map((item: number) => item + 1);
  return rows.filter(Boolean);
};
