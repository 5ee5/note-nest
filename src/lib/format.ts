const dateFmt = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });

export function formatDate(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : dateFmt.format(d);
}
