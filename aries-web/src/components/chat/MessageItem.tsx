import { User, Bot, Copy, Check, AlertCircle, Loader2 } from 'lucide-react';
import { useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkEmoji from 'remark-emoji';
import remarkMath from 'remark-math';
import rehypeKatex from 'rehype-katex';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark, oneLight } from 'react-syntax-highlighter/dist/esm/styles/prism';
import Zoom from 'react-medium-image-zoom';
import 'katex/dist/katex.min.css';
import 'react-medium-image-zoom/dist/styles.css';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { UIMessage, UISubAgent } from '@/api/types';
import type { ExecutionStatus } from '@/stores';
import { ThinkingProcess } from './ThinkingProcess';
import { MermaidDiagram } from './MermaidDiagram';

interface MessageItemProps {
  message: UIMessage;
  executionStatus?: ExecutionStatus;
  isStreaming?: boolean;
  subAgents?: UISubAgent[];
  getSubAgent?: (id: string) => UISubAgent | undefined;
}

export function MessageItem({ message, executionStatus, isStreaming, subAgents, getSubAgent }: MessageItemProps) {
  const [copied, setCopied] = useState(false);
  const [copiedCode, setCopiedCode] = useState<string | null>(null);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(message.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleCopyCode = async (code: string) => {
    await navigator.clipboard.writeText(code);
    setCopiedCode(code);
    setTimeout(() => setCopiedCode(null), 2000);
  };

  const isUser = message.role === 'user';
  const isDark = typeof document !== 'undefined' && document.documentElement.classList.contains('dark');

  return (
    <div
      className={cn(
        'flex w-full mb-8 animate-in-up group',
        isUser ? 'justify-end' : 'justify-start'
      )}
    >
      <div
        className={cn(
          'flex gap-4 w-fit max-w-full',
          isUser ? 'flex-row-reverse items-start max-w-[85%]' : 'flex-row items-start'
        )}
      >
        {/* Avatar */}
        <div
          className={cn(
            'shrink-0 w-8 h-8 rounded-full flex items-center justify-center border',
            isUser
              ? 'bg-background border-border text-foreground shadow-sm'
              : 'bg-background border-border text-foreground shadow-sm'
          )}
        >
          {isUser ? <User className="h-4 w-4" /> : <Bot className="h-4 w-4" />}
        </div>

        {/* Message Content Area */}
        <div className={cn("flex flex-col gap-2", isUser ? "items-end" : "items-start flex-1")}>
          <div
            className={cn(
              'transition-all duration-300 w-fit',
              isUser
                ? 'bg-secondary text-secondary-foreground px-4 py-2.5 rounded-2xl'
                : 'bg-transparent px-0 py-1' // Assistant has no bubble
            )}
          >
            {/* Content Container */}
            <div className="space-y-4">
              {/* Error state */}
              {message.error && (
                <div className="flex items-center gap-2 text-destructive bg-destructive/5 px-3 py-2 rounded-lg border border-destructive/20 text-sm">
                  <AlertCircle className="h-4 w-4" />
                  <span>{message.error}</span>
                </div>
              )}

              {/* Thinking process (only for assistant) */}
              {!isUser && (message.thinking || message.toolCalls?.length || message.executionEvents?.length || message.taskPlan || subAgents?.length || (message.isStreaming && executionStatus)) && (
                <div className="mb-4">
                  <ThinkingProcess
                    thinking={message.thinking}
                    toolCalls={message.toolCalls}
                    executionEvents={message.executionEvents}
                    taskPlan={message.taskPlan}
                    executionStatus={message.isStreaming ? executionStatus : undefined}
                    isStreaming={message.isStreaming && isStreaming}
                    subAgents={subAgents}
                    getSubAgent={getSubAgent}
                  />
                </div>
              )}

              {/* Main content */}
              {message.content && (
                message.content === 'Interrupted by user.' ? (
                  // Special styling for interrupted message
                  <p className="text-sm italic text-muted-foreground">
                    {message.content}
                  </p>
                ) : (
                <div className={cn(
                  "prose prose-sm max-w-none leading-relaxed",
                  isUser ? "text-foreground" : "dark:prose-invert text-foreground/90 font-sans"
                )}>
                  <ReactMarkdown
                    remarkPlugins={[remarkGfm, remarkEmoji, remarkMath]}
                    rehypePlugins={[rehypeKatex]}
                    components={{
                      // Custom code block rendering with syntax highlighting
                      code({ className, children, ...props }) {
                        const match = /language-(\w+)/.exec(className || '');
                        const language = match?.[1] || '';
                        const codeString = String(children).replace(/\n$/, '');
                        const isInline = !match && !codeString.includes('\n');

                        if (isInline) {
                          return (
                            <code
                              className={cn(
                                "px-1.5 py-0.5 rounded-md text-sm font-mono font-medium bg-muted text-primary"
                              )}
                              {...props}
                            >
                              {children}
                            </code>
                          );
                        }

                        // Mermaid diagram support
                        if (language === 'mermaid') {
                          return <MermaidDiagram chart={codeString} />;
                        }

                        return (
                          <div className="relative group/code my-6 rounded-xl overflow-hidden border border-border/50 shadow-sm min-w-[200px]">
                            <div className="flex items-center justify-between px-4 py-2 bg-muted/80 backdrop-blur-sm border-b border-border/50">
                              <span className="text-xs font-mono text-muted-foreground uppercase">{language || 'code'}</span>
                              <Button
                                variant="ghost"
                                size="icon"
                                className="h-7 w-7 hover:bg-background/80 transition-colors"
                                onClick={() => handleCopyCode(codeString)}
                              >
                                {copiedCode === codeString ? (
                                  <Check className="h-3.5 w-3.5 text-green-500" />
                                ) : (
                                  <Copy className="h-3.5 w-3.5" />
                                )}
                              </Button>
                            </div>
                            <SyntaxHighlighter
                              style={isDark ? oneDark : oneLight}
                              language={language || 'text'}
                              PreTag="div"
                              customStyle={{
                                margin: 0,
                                padding: '1rem',
                                background: 'transparent',
                                fontSize: '13.5px',
                                lineHeight: '1.6',
                              }}
                              codeTagProps={{
                                style: {
                                  fontFamily: "'JetBrains Mono', 'Fira Code', 'Roboto Mono', monospace",
                                },
                              }}
                            >
                              {codeString}
                            </SyntaxHighlighter>
                          </div>
                        );
                      },
                      // Image with zoom
                      img({ src, alt, ...props }) {
                        return (
                          <Zoom>
                            <img
                              src={src}
                              alt={alt || ''}
                              className="rounded-lg max-w-full h-auto cursor-zoom-in"
                              {...props}
                            />
                          </Zoom>
                        );
                      },
                      // Enhanced table styling
                      table({ children, ...props }) {
                        return (
                          <div className="my-4 overflow-x-auto rounded-lg border border-border">
                            <table className="min-w-full divide-y divide-border" {...props}>
                              {children}
                            </table>
                          </div>
                        );
                      },
                      thead({ children, ...props }) {
                        return (
                          <thead className="bg-muted/50" {...props}>
                            {children}
                          </thead>
                        );
                      },
                      th({ children, ...props }) {
                        return (
                          <th
                            className="px-4 py-2.5 text-left text-xs font-semibold text-foreground uppercase tracking-wider"
                            {...props}
                          >
                            {children}
                          </th>
                        );
                      },
                      td({ children, ...props }) {
                        return (
                          <td
                            className="px-4 py-2.5 text-sm text-foreground/90 border-t border-border"
                            {...props}
                          >
                            {children}
                          </td>
                        );
                      },
                      // Enhanced link styling
                      a({ href, children, ...props }) {
                        const isExternal = href?.startsWith('http');
                        return (
                          <a
                            href={href}
                            target={isExternal ? '_blank' : undefined}
                            rel={isExternal ? 'noopener noreferrer' : undefined}
                            className="text-primary hover:text-primary/80 underline underline-offset-2 transition-colors"
                            {...props}
                          >
                            {children}
                          </a>
                        );
                      },
                    }}
                  >
                    {message.content}
                  </ReactMarkdown>
                </div>
                )
              )}

              {/* Streaming indicator */}
              {message.isStreaming && !message.content && (
                <div className="flex items-center gap-2.5 text-muted-foreground py-1">
                  <Loader2 className="h-4 w-4 animate-spin text-primary" />
                  <span className="text-sm font-medium animate-pulse">正在思考中...</span>
                </div>
              )}
            </div>
          </div>

          {/* Action buttons for assistant messages - bottom left */}
          {!isUser && message.content && !message.isStreaming && (
            <div className="opacity-0 group-hover:opacity-100 transition-opacity flex justify-start gap-1 w-full mt-2">
              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8 hover:bg-muted/80 rounded-lg text-muted-foreground transition-all"
                onClick={handleCopy}
                title="复制消息"
              >
                {copied ? <Check className="h-4 w-4 text-green-500" /> : <Copy className="h-4 w-4" />}
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
