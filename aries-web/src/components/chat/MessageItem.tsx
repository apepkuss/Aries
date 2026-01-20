import { User, Bot, Copy, Check, AlertCircle, Loader2 } from 'lucide-react';
import { useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { UIMessage } from '@/api/types';
import type { ExecutionStatus } from '@/stores';
import { ThinkingProcess } from './ThinkingProcess';

interface MessageItemProps {
  message: UIMessage;
  executionStatus?: ExecutionStatus;
  isStreaming?: boolean;
}

export function MessageItem({ message, executionStatus, isStreaming }: MessageItemProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(message.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const isUser = message.role === 'user';

  return (
    <div
      className={cn(
        'flex gap-3 p-4 rounded-lg',
        isUser ? 'bg-muted/50' : 'bg-background'
      )}
    >
      {/* Avatar */}
      <div
        className={cn(
          'shrink-0 w-8 h-8 rounded-full flex items-center justify-center',
          isUser ? 'bg-primary text-primary-foreground' : 'bg-muted'
        )}
      >
        {isUser ? <User className="h-4 w-4" /> : <Bot className="h-4 w-4" />}
      </div>

      {/* Content */}
      <div className="flex-1 space-y-2 overflow-hidden">
        {/* Error state */}
        {message.error && (
          <div className="flex items-center gap-2 text-destructive text-sm">
            <AlertCircle className="h-4 w-4" />
            <span>{message.error}</span>
          </div>
        )}

        {/* Thinking process - shows thoughts, tool calls, task plan, and execution status in timeline */}
        {!isUser && (message.thinking || message.toolCalls?.length || message.executionEvents?.length || message.taskPlan || (message.isStreaming && executionStatus)) && (
          <ThinkingProcess
            thinking={message.thinking}
            toolCalls={message.toolCalls}
            executionEvents={message.executionEvents}
            taskPlan={message.taskPlan}
            executionStatus={message.isStreaming ? executionStatus : undefined}
            isStreaming={message.isStreaming && isStreaming}
          />
        )}

        {/* Main content */}
        {message.content && (
          <div className="prose prose-sm dark:prose-invert max-w-none">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                // Custom code block rendering
                code({ className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || '');
                  const isInline = !match;

                  if (isInline) {
                    return (
                      <code
                        className="bg-muted px-1.5 py-0.5 rounded text-sm font-mono"
                        {...props}
                      >
                        {children}
                      </code>
                    );
                  }

                  return (
                    <div className="relative group">
                      <div className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity">
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6"
                          onClick={() => {
                            navigator.clipboard.writeText(String(children));
                          }}
                        >
                          <Copy className="h-3 w-3" />
                        </Button>
                      </div>
                      <pre className="bg-muted p-4 rounded-md overflow-x-auto">
                        <code className={className} {...props}>
                          {children}
                        </code>
                      </pre>
                    </div>
                  );
                },
              }}
            >
              {message.content}
            </ReactMarkdown>
          </div>
        )}

        {/* Streaming indicator */}
        {message.isStreaming && !message.content && (
          <div className="flex items-center gap-2 text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            <span className="text-sm">Thinking...</span>
          </div>
        )}
      </div>

      {/* Copy button for assistant messages */}
      {!isUser && message.content && !message.isStreaming && (
        <Button
          variant="ghost"
          size="icon"
          className="shrink-0 h-8 w-8 opacity-0 hover:opacity-100 focus:opacity-100"
          onClick={handleCopy}
        >
          {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
        </Button>
      )}
    </div>
  );
}
