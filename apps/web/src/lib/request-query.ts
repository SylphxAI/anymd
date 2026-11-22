// Match the public page contract: singleton parameters are ignored if repeated.
export function single(value: string | string[] | undefined) {
  return typeof value === 'string' ? value : undefined;
}
