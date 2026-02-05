import { MessageList } from './MessageList';
import { ChatInput } from './ChatInput';
import { HitlOverlay } from '@/components/hitl';

export function ChatContainer() {
  return (
    <div className="h-full flex flex-col overflow-hidden">
      <MessageList />
      <HitlOverlay className="shrink-0 px-4 pb-2 max-w-4xl mx-auto w-full" />
      <ChatInput />
    </div>
  );
}
