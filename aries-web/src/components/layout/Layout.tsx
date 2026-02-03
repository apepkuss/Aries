import type { ReactNode } from 'react';
import { Header } from './Header';
import { Sidebar } from './Sidebar';
import { SkillsPage } from '@/components/skills';
import { useUIStore } from '@/stores';

interface LayoutProps {
  children: ReactNode;
}

export function Layout({ children }: LayoutProps) {
  const activeView = useUIStore((s) => s.activeView);

  return (
    <div className="h-screen flex flex-col">
      <Header />
      <div className="flex-1 flex overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-hidden">
          {activeView === 'skills' ? <SkillsPage /> : children}
        </main>
      </div>
    </div>
  );
}
