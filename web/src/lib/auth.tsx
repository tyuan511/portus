import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { api } from "@/lib/api";
import type { Identity, LoginResponse, Permission } from "@/types";

interface AuthState {
  identity: Identity | null;
  loading: boolean;
  mustChangePassword: boolean;
  login: (username: string, password: string, totpCode?: string) => Promise<LoginResponse>;
  logout: () => Promise<void>;
  refresh: () => Promise<void>;
  setMustChangePassword: (v: boolean) => void;
  hasPermission: (p: Permission) => boolean;
  isAdmin: boolean;
  hasTotp: boolean;
}

const AuthContext = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [identity, setIdentity] = useState<Identity | null>(null);
  const [loading, setLoading] = useState(true);
  const [mustChangePassword, setMustChange] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const data = await api.get<{ user: Identity }>("/api/auth/me");
      setIdentity(data.user);
      setMustChange(false);
    } catch {
      setIdentity(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const login = useCallback(async (username: string, password: string, totpCode?: string) => {
    const data = await api.post<LoginResponse>("/api/auth/login", {
      username,
      password,
      totp_code: totpCode || "",
    });
    setIdentity(data.user);
    setMustChange(data.must_change_password);
    return data;
  }, []);

  const logout = useCallback(async () => {
    try {
      await api.post("/api/auth/logout");
    } finally {
      setIdentity(null);
      setMustChange(false);
    }
  }, []);

  const value = useMemo<AuthState>(
    () => ({
      identity,
      loading,
      mustChangePassword,
      login,
      logout,
      refresh,
      setMustChangePassword: setMustChange,
      hasPermission: (p) => identity?.permissions.includes(p) ?? false,
      isAdmin: identity?.is_admin ?? false,
      hasTotp: identity?.totp_enabled ?? false,
    }),
    [identity, loading, mustChangePassword, login, logout, refresh]
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth outside AuthProvider");
  return ctx;
}
