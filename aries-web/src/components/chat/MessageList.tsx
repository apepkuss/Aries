import { useEffect, useRef } from 'react';
import { ScrollArea } from '@/components/ui/scroll-area';
import { MessageItem } from './MessageItem';
import { useChatStore } from '@/stores';

export function MessageList() {
  const { messages, executionStatus, isStreaming } = useChatStore();
  const bottomRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  if (messages.length === 0) {
    return (
      <div className="flex-1 min-h-0 flex items-center justify-center">
        <div className="text-center text-muted-foreground">
          <p className="text-lg font-medium">Welcome to Aries</p>
          <p className="text-sm">Start a conversation by typing a message below</p>
        </div>
      </div>
    );
  }

  // Find the last streaming message to pass execution status to
  let lastStreamingIndex = -1;
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].isStreaming) {
      lastStreamingIndex = i;
      break;
    }
  }

  return (
    <ScrollArea className="flex-1 min-h-0">
      <div className="max-w-4xl mx-auto py-4 px-4 space-y-2">
        {messages.map((message, index) => (
          <MessageItem
            key={message.id}
            message={message}
            executionStatus={index === lastStreamingIndex ? executionStatus : undefined}
            isStreaming={index === lastStreamingIndex ? isStreaming : false}
          />
        ))}
        <div ref={bottomRef} />
      </div>
    </ScrollArea>
  );
}
