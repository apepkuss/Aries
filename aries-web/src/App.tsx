import { Toaster } from '@/components/ui/sonner';
import { Layout } from '@/components/layout';
import { ChatContainer } from '@/components/chat';
import { ConfigPanel } from '@/components/settings';
import { useTheme } from '@/hooks';

function App() {
  // Apply theme
  useTheme();

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
