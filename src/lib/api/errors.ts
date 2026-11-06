/** Error raised by the notes API. `code` is stable and safe to branch on. */
export class ApiError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }
}

/** Normalizes anything thrown (including the `{code, message}` objects Tauri rejects with). */
export function toApiError(e: unknown): ApiError {
  if (e instanceof ApiError) return e;
  if (typeof e === "object" && e !== null && "code" in e && "message" in e) {
    const { code, message } = e as { code: unknown; message: unknown };
    if (typeof code === "string" && typeof message === "string") return new ApiError(code, message);
  }
  if (e instanceof Error) return new ApiError("unknown", e.message);
  return new ApiError("unknown", String(e));
}
