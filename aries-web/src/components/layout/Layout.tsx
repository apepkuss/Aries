import type { ReactNode } from 'react';
import { Header } from './Header';
import { ActivityBar } from './ActivityBar';
import { Sidebar } from './Sidebar';
import { SkillsPage } from '@/components/skills';
import { McpPage } from '@/components/mcp';
import { useUIStore } from '@/stores';

interface LayoutProps {
  children: ReactNode;
}

export function Layout({ children }: LayoutProps) {
  const activeView = useUIStore((s) => s.activeView);

  const renderContent = () => {
    switch (activeView) {
      case 'skills':
        return <SkillsPage />;
      case 'mcp':
        return <McpPage />;
      default:
        return children;
    }
  };

  return (
    <div className="h-screen flex flex-col">
      <Header />
      <div className="flex-1 flex overflow-hidden">
        <ActivityBar />
        <Sidebar />
        <main className="flex-1 overflow-hidden">
          {renderContent()}
        </main>
      </div>
    </div>
  );
}
