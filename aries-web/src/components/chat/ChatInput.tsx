import { useState, useRef, useEffect, type KeyboardEvent } from 'react';
import { Send, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { useChatStore } from '@/stores';
import { ExecutionModeSelector } from './ExecutionModeSelector';

export function ChatInput() {
  const [input, setInput] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const { sendMessage, stopGeneration, isStreaming } = useChatStore();

  // Auto-focus on mount
  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  // Re-focus when streaming ends (answer received)
  useEffect(() => {
    if (!isStreaming) {
      // Small delay to ensure UI updates are complete
      const timer = setTimeout(() => {
        textareaRef.current?.focus();
      }, 100);
      return () => clearTimeout(timer);
    }
  }, [isStreaming]);

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) {
      textarea.style.height = 'auto';
      textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`;
    }
  }, [input]);

  // Handle send
  const handleSend = () => {
    if (!input.trim() || isStreaming) return;
    sendMessage(input);
    setInput('');
  };

  // Handle key press
  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Cmd/Ctrl + Enter to send
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      handleSend();
    }
    // Escape to stop generation
    if (e.key === 'Escape' && isStreaming) {
      stopGeneration();
    }
  };

  return (
    <div className="border-t p-4 shrink-0 bg-background">
      <div className="max-w-4xl mx-auto space-y-2">
        <div className="flex gap-2 items-end">
          <div className="flex-1 relative">
            <Textarea
              ref={textareaRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Type a message... (Cmd+Enter to send)"
              className="min-h-[44px] max-h-[200px] resize-none pr-12"
              rows={1}
              disabled={isStreaming}
            />
          </div>

          {isStreaming ? (
            <Button
              variant="destructive"
              size="icon"
              onClick={stopGeneration}
              title="Stop generation (Esc)"
            >
              <Square className="h-4 w-4" />
            </Button>
          ) : (
            <Button
              size="icon"
              onClick={handleSend}
              disabled={!input.trim()}
              title="Send message (Cmd+Enter)"
            >
              <Send className="h-4 w-4" />
            </Button>
          )}
        </div>

        {/* Execution Mode Selector */}
        <div className="flex items-center justify-between">
          <ExecutionModeSelector />
          <span className="text-xs text-muted-foreground">
            Cmd+Enter to send
          </span>
        </div>
      </div>
    </div>
  );
}
