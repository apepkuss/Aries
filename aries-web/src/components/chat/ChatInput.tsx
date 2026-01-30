import { useState, useRef, useEffect, type KeyboardEvent } from 'react';
import { ArrowUp, Square, Shield, ShieldCheck } from 'lucide-react';
import { useChatStore, useUIStore } from '@/stores';
import { ExecutionModeSelector } from './ExecutionModeSelector';
import { ModelSelector } from './ModelSelector';

export function ChatInput() {
  const [input, setInput] = useState('');
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const { sendMessage, stopGeneration, isStreaming } = useChatStore();
  const { privacyMode } = useUIStore();

  // Auto-focus on mount
  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  // Re-focus when streaming ends (answer received)
  useEffect(() => {
    if (!isStreaming) {
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

  const handleSend = () => {
    if (!input.trim() || isStreaming) return;
    sendMessage(input);
    setInput('');
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
    if (e.key === 'Escape' && isStreaming) {
      stopGeneration();
    }
  };

  // Focus the textarea when clicking the container background
  const handleContainerClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) {
      textareaRef.current?.focus();
    }
  };

  const canSend = input.trim().length > 0 && !isStreaming;

  return (
    <div className="p-4 shrink-0 bg-background">
      <div className="max-w-4xl mx-auto">
        <div
          className="rounded-lg border border-border/60 bg-muted/30 shadow-sm focus-within:border-border focus-within:ring-1 focus-within:ring-ring/20 transition-[border-color,box-shadow]"
          onClick={handleContainerClick}
        >
          {/* Textarea */}
          <div className="px-4 pt-3 pb-1">
            <textarea
              ref={textareaRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Type a message..."
              className="w-full min-h-[28px] max-h-[200px] resize-none bg-transparent text-sm outline-none placeholder:text-muted-foreground/60 disabled:cursor-not-allowed disabled:opacity-50"
              rows={1}
              disabled={isStreaming}
            />
          </div>

          {/* Toolbar */}
          <div className="flex items-center justify-between px-3 pb-2.5 pt-1">
            {/* Left: controls */}
            <div className="flex items-center gap-1">
              <div
                className="flex items-center justify-center w-7 h-7 rounded-md hover:bg-muted/80 transition-colors cursor-default"
                title={privacyMode ? 'Privacy mode enabled' : 'Privacy mode disabled'}
              >
                {privacyMode ? (
                  <ShieldCheck className="h-[18px] w-[18px] text-emerald-500 fill-emerald-500/20" />
                ) : (
                  <Shield className="h-[18px] w-[18px] text-muted-foreground/50" />
                )}
              </div>
              <ExecutionModeSelector />
              <ModelSelector />
            </div>

            {/* Right: send / stop button */}
            {isStreaming ? (
              <button
                onClick={stopGeneration}
                title="Stop generation (Esc)"
                className="flex items-center justify-center w-8 h-8 rounded-full bg-destructive text-white hover:bg-destructive/90 transition-colors"
              >
                <Square className="h-3.5 w-3.5" />
              </button>
            ) : (
              <button
                onClick={handleSend}
                disabled={!canSend}
                title="Send message (Enter)"
                className="flex items-center justify-center w-8 h-8 rounded-full bg-primary text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-30 disabled:cursor-not-allowed"
              >
                <ArrowUp className="h-[18px] w-[18px]" strokeWidth={2.5} />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
