import { useQuery, useQueryClient } from '@tanstack/react-query';
import type { ConnectionStatus } from '@gitexplore/api-client';
import { createContext, type ReactNode, useContext } from 'react';

import { api } from './api';

type AuthContextValue = {
  status: ConnectionStatus | undefined;
  loading: boolean;
  error: Error | null;
  refresh: () => Promise<unknown>;
};

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const query = useQuery({
    queryKey: ['auth-status'],
    queryFn: () => api.getAuthStatus(),
    retry: false,
    staleTime: 30_000,
  });

  return (
    <AuthContext.Provider
      value={{
        status: query.data,
        loading: query.isPending,
        error: query.error instanceof Error ? query.error : null,
        refresh: query.refetch,
      }}
    >
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const value = useContext(AuthContext);
  if (!value) throw new Error('useAuth must be used inside AuthProvider');
  return value;
}

export function useLogout() {
  const queryClient = useQueryClient();

  return async () => {
    await api.logout();
    queryClient.setQueryData<ConnectionStatus>(['auth-status'], {
      authenticated: false,
      app_user_id: null,
      connected: false,
      account: null,
    });
    queryClient.clear();
    window.location.assign('/login');
  };
}
