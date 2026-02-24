import type { FileAttachment } from '@/api/types';

interface ElectronAPI {
  platform: string;
  isElectron: boolean;
  showNotification: (title: string, body: string) => void;
  selectFiles?: () => Promise<FileAttachment[]>;
}

function getElectronAPI(): ElectronAPI | undefined {
  return (window as unknown as Record<string, unknown>)?.electronAPI as ElectronAPI | undefined;
}

/**
 * Show a native system notification (Electron) or browser notification (web).
 * Only sends when the window is not focused, to avoid duplicating in-app toasts.
 */
export function showNativeNotification(title: string, body: string): void {
  if (document.hasFocus()) {
    return;
  }

  const api = getElectronAPI();
  if (api?.showNotification) {
    api.showNotification(title, body);
  } else if ('Notification' in window && Notification.permission === 'granted') {
    new Notification(title, { body });
  }
}

/**
 * Request notification permission (browser only, Electron doesn't need it).
 */
export function requestNotificationPermission(): void {
  const api = getElectronAPI();
  if (!api && 'Notification' in window && Notification.permission === 'default') {
    Notification.requestPermission();
  }
}
