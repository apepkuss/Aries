import { useEffect } from 'react';
import { Toaster } from '@/components/ui/sonner';
import { Layout } from '@/components/layout';
import { ChatContainer } from '@/components/chat';
import { ConfigPanel } from '@/components/settings';
import { useTheme } from '@/hooks';
import { useConfigStore, useServiceStore } from '@/stores';

function App() {
  // Apply theme
  useTheme();

  // Load config on app startup
  const fetchConfig = useConfigStore((state) => state.fetchConfig);
  const autoRegister = useServiceStore((state) => state.autoRegister);
  useEffect(() => {
    fetchConfig();
    autoRegister();
  }, [fetchConfig, autoRegister]);

  return (
    <>
      <Layout>
        <ChatContainer />
      </Layout>
      <ConfigPanel />
      <Toaster position="bottom-right" />
    </>
  );
}

export default App;
