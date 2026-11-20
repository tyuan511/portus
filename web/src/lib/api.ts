// Unified API client: every /api call unwraps the {code,message,data} envelope.
// Cookies ride along automatically (same-origin), no token plumbing needed.

export class ApiError extends Error {
  code: number;
  status: number;
  constructor(code: number, status: number, message: string) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

export const TOTP_REQUIRED = 41003;

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const resp = await fetch(path, {
    method,
    credentials: "same-origin",
    headers: body !== undefined ? { "Content-Type": "application/json" } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  let envelope: { code?: number; message?: string; data?: T } = {};
  try {
    envelope = await resp.json();
  } catch {
    /* non-JSON error page */
  }

  if (!resp.ok) {
    throw new ApiError(envelope.code ?? 50001, resp.status, envelope.message ?? resp.statusText);
  }
  if ((envelope.code ?? 0) !== 0) {
    throw new ApiError(envelope.code ?? 50001, resp.status, envelope.message ?? "request failed");
  }
  return envelope.data as T;
}

export const api = {
  get: <T>(path: string) => request<T>("GET", path),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {}),
  put: <T>(path: string, body?: unknown) => request<T>("PUT", path, body ?? {}),
  patch: <T>(path: string, body?: unknown) => request<T>("PATCH", path, body ?? {}),
  del: <T>(path: string) => request<T>("DELETE", path),
};

// --- WebSocket URL helper (cookie auth; no token in URLs) ---
export function wsUrl(path: string): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}${path}`;
}
