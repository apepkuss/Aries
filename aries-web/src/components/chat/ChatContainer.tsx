import { MessageList } from './MessageList';
import { ChatInput } from './ChatInput';

export function ChatContainer() {
  return (
    <div className="h-full flex flex-col overflow-hidden">
      <MessageList />
      <ChatInput />
    </div>
  );
}
