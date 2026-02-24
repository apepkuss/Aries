import { useEffect, useRef } from 'react';
import { Loader2 } from 'lucide-react';
import { ScrollArea } from '@/components/ui/scroll-area';
import { MessageItem } from './MessageItem';
import { useChatStore, useSessionsStore } from '@/stores';

export function MessageList() {
  const { messages, executionStatus, isStreaming, getSubAgent, getRootSubAgents } = useChatStore();
  const isLoadingDetail = useSessionsStore((s) => s.isLoadingDetail);
  const bottomRef = useRef<HTMLDivElement>(null);
  const rootSubAgents = getRootSubAgents();

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  if (isLoadingDetail) {
    return (
      <div className="flex-1 min-h-0 flex items-center justify-center">
        <div className="text-center text-muted-foreground">
          <Loader2 className="h-5 w-5 animate-spin mx-auto mb-2" />
          <p className="text-sm">加载对话记录...</p>
        </div>
      </div>
    );
  }

  if (messages.length === 0) {
    return (
      <div className="flex-1 min-h-0 flex items-center justify-center">
        <div className="text-center text-muted-foreground">
          <p className="text-lg font-medium">老板好，我是小苔藓，有事儿请吩咐！</p>
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
      <div className="w-full max-w-4xl mx-auto py-4 px-4 space-y-2">
        {messages.map((message, index) => (
          <MessageItem
            key={message.id}
            message={message}
            executionStatus={index === lastStreamingIndex ? executionStatus : undefined}
            isStreaming={index === lastStreamingIndex ? isStreaming : false}
            subAgents={index === lastStreamingIndex ? rootSubAgents : undefined}
            getSubAgent={index === lastStreamingIndex ? getSubAgent : undefined}
          />
        ))}
        <div ref={bottomRef} />
      </div>
    </ScrollArea>
  );
}
