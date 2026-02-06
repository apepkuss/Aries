import { useState, useRef, useEffect, type KeyboardEvent } from 'react';
import { ArrowUp, Square, Paperclip, X, FileText, FileImage, FileCode, File, ShieldCheck } from 'lucide-react';
import { useChatStore } from '@/stores';
import { isElectron } from '@/api/client';
import type { FileAttachment } from '@/api/types';
import { ExecutionModeSelector } from './ExecutionModeSelector';
import { ModelSelector } from './ModelSelector';
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

// Format file size for display
function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// Get icon component based on MIME type
function getFileIcon(mimeType: string) {
  if (mimeType.startsWith('image/')) return FileImage;
  if (mimeType.startsWith('text/x-') || mimeType === 'text/javascript' || mimeType === 'text/typescript' || mimeType === 'text/css' || mimeType === 'text/html') return FileCode;
  if (mimeType.startsWith('text/') || mimeType === 'application/json' || mimeType === 'application/xml') return FileText;
  return File;
}

export function ChatInput() {
  const [input, setInput] = useState('');
  const [attachments, setAttachments] = useState<FileAttachment[]>([]);
  const [privacyDeclare, setPrivacyDeclare] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const { sendMessage, stopGeneration, isStreaming } = useChatStore();

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

  const handleSelectFiles = async () => {
    const api = (window as unknown as Record<string, unknown>)?.electronAPI as { selectFiles?: () => Promise<FileAttachment[]> } | undefined;
    if (!api?.selectFiles) return;
    try {
      const files = await api.selectFiles();
      if (files.length > 0) {
        setAttachments((prev) => [...prev, ...files]);
      }
    } catch {
      // User cancelled or error — silently ignore
    }
  };

  const handleRemoveAttachment = (index: number) => {
    setAttachments((prev) => prev.filter((_, i) => i !== index));
  };

  const handleSend = async () => {
    const hasContent = input.trim().length > 0;
    const hasAttachments = attachments.length > 0;
    if (isStreaming || (!hasContent && !hasAttachments)) return;

    // Validate attached files still exist before sending
    if (hasAttachments) {
      const api = (window as unknown as Record<string, unknown>)?.electronAPI as { checkFilesExist?: (paths: string[]) => Promise<boolean[]> } | undefined;
      if (api?.checkFilesExist) {
        try {
          const exists = await api.checkFilesExist(attachments.map((f) => f.path));
          const missing = attachments.filter((_, i) => !exists[i]);
          if (missing.length > 0) {
            const remaining = attachments.filter((_, i) => exists[i]);
            setAttachments(remaining);
            const names = missing.map((f) => f.name).join(', ');
            const canStillSend = remaining.length > 0 || hasContent;
            if (!canStillSend) {
              alert(`The following files no longer exist and have been removed:\n${names}`);
              return;
            }
            if (!confirm(`The following files no longer exist and have been removed:\n${names}\n\nContinue sending?`)) {
              return;
            }
            sendMessage(input, remaining.length > 0 ? remaining : undefined, { privacyMode: privacyDeclare || undefined });
            setInput('');
            setAttachments([]);
            setPrivacyDeclare(false);
            return;
          }
        } catch {
          // If check fails, proceed with send — backend will handle missing files
        }
      }
    }

    sendMessage(input, hasAttachments ? attachments : undefined, { privacyMode: privacyDeclare || undefined });
    setInput('');
    setAttachments([]);
    setPrivacyDeclare(false);
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

  const canSend = (input.trim().length > 0 || attachments.length > 0) && !isStreaming;

  return (
    <div className="p-4 shrink-0 bg-background">
      <div className="max-w-4xl mx-auto">
        <div
          className={cn(
            "rounded-lg border bg-muted/30 shadow-sm transition-[border-color,box-shadow]",
            privacyDeclare
              ? "border-emerald-500/60 ring-1 ring-emerald-500/20"
              : "border-border/60 focus-within:border-border focus-within:ring-1 focus-within:ring-ring/20"
          )}
          onClick={handleContainerClick}
        >
          {/* Attachment preview area */}
          {attachments.length > 0 && (
            <div className="px-3 pt-3 pb-1 flex flex-wrap gap-2">
              {attachments.map((file, index) => {
                const Icon = getFileIcon(file.mimeType);
                return (
                  <div
                    key={`${file.path}-${index}`}
                    className="flex items-center gap-1.5 rounded-md border border-border/60 bg-background px-2.5 py-1.5 text-xs group"
                  >
                    <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                    <span className="max-w-[150px] truncate" title={file.name}>
                      {file.name}
                    </span>
                    <span className="text-muted-foreground/60">{formatFileSize(file.size)}</span>
                    <button
                      onClick={() => handleRemoveAttachment(index)}
                      className="ml-0.5 rounded-sm p-0.5 text-muted-foreground/60 hover:text-foreground hover:bg-muted transition-colors"
                      title="Remove"
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </div>
                );
              })}
            </div>
          )}

          {/* Textarea with privacy button */}
          <div className="flex items-start gap-1.5 pl-3 pr-4 pt-3 pb-1">
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  onClick={() => setPrivacyDeclare((prev) => !prev)}
                  disabled={isStreaming}
                  className={cn(
                    "flex items-center justify-center w-5 h-5 shrink-0 rounded transition-colors disabled:opacity-50 disabled:cursor-not-allowed",
                    privacyDeclare
                      ? "text-emerald-500 hover:text-emerald-400"
                      : "text-muted-foreground hover:text-foreground"
                  )}
                >
                  <ShieldCheck className="h-4 w-4" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="top">
                {privacyDeclare ? '隐私声明已开启 — 此消息将使用隐私模式发送' : '声明此消息为隐私内容'}
              </TooltipContent>
            </Tooltip>
            <textarea
              ref={textareaRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Type a message..."
              className="w-full min-h-[20px] max-h-[200px] resize-none bg-transparent text-sm leading-5 outline-none placeholder:text-muted-foreground/60 disabled:cursor-not-allowed disabled:opacity-50"
              rows={1}
              disabled={isStreaming}
            />
          </div>

          {/* Toolbar */}
          <div className="flex items-center justify-between pl-1.5 pr-3 pb-2.5 pt-1">
            {/* Left: controls */}
            <div className="flex items-center gap-1">
              {isElectron() && (
                <button
                  onClick={handleSelectFiles}
                  disabled={isStreaming}
                  title="Attach files"
                  className="flex items-center justify-center w-8 h-8 rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <Paperclip className="h-4 w-4" />
                </button>
              )}
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
