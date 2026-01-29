import { useState, useRef, useEffect, type KeyboardEvent } from 'react';
import { Send, Square, Shield, ShieldCheck } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { useChatStore } from '@/stores';
import { ExecutionModeSelector } from './ExecutionModeSelector';
import { ModelSelector } from './ModelSelector';

export function ChatInput() {
  const [input, setInput] = useState('');
  const [privacyMode, setPrivacyMode] = useState(false);
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
              placeholder={privacyMode ? "Type a message... (Privacy mode enabled)" : "Type a message..."}
              className="min-h-[44px] max-h-[200px] resize-none pl-10 pr-3"
              rows={1}
              disabled={isStreaming}
            />
            <button
              type="button"
              onClick={() => setPrivacyMode(!privacyMode)}
              className="absolute left-1 top-1/2 -translate-y-1/2 p-1 rounded-md transition-all duration-200 hover:bg-muted/80 focus:outline-none focus:ring-1 focus:ring-primary/50"
              title={privacyMode ? 'Privacy mode enabled' : 'Enable privacy mode'}
            >
{privacyMode ? (
                <ShieldCheck
                  className="h-6 w-6 text-emerald-500 fill-emerald-500/20 transition-all duration-200"
                />
              ) : (
                <Shield
                  className="h-6 w-6 text-muted-foreground/50 hover:text-muted-foreground transition-all duration-200"
                />
              )}
            </button>
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

        {/* Mode and Model Selectors */}
        <div className="flex items-center">
          <div className="flex items-center gap-4">
            <ExecutionModeSelector />
            <ModelSelector />
          </div>
        </div>
      </div>
    </div>
  );
}
