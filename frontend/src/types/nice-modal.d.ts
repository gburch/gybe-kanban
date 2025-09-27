import type { ComponentType, ReactNode } from 'react';

declare module '@ebay/nice-modal-react' {
  export interface NiceModalInstance<TArgs = unknown> {
    id: string;
    args: TArgs;
    visible: boolean;
    show: (args?: Partial<TArgs>) => Promise<unknown>;
    hide: () => void;
    remove: () => void;
    resolve: (value?: unknown) => void;
    reject: (reason?: unknown) => void;
  }

  export function useModal<TArgs = unknown>(): NiceModalInstance<TArgs>;

  export function create<TProps>(
    component: ComponentType<TProps>
  ): ComponentType<TProps>;

  interface NiceModalApi {
    Provider: ComponentType<{ children: ReactNode }>;
    register: (id: string, component: ComponentType<any>) => void;
    show: <TArgs = unknown>(id: string, args?: TArgs) => Promise<unknown>;
    hide: (id: string) => void;
    remove: (id: string) => void;
    create: typeof create;
  }

  const NiceModal: NiceModalApi;
  export default NiceModal;
}
