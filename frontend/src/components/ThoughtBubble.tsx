import { useState, useEffect, useRef, useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import { Brain, Loader2, CheckCircle2 } from 'lucide-react';
import { cn } from '@/lib/utils';

interface ThoughtBubbleProps {
  /** The thought content to display */
  content: string;
  /** Whether this thought is still being generated (streaming) */
  isStreaming?: boolean;
  /** Whether this thought is complete */
  isComplete?: boolean;
  /** Iteration number for the thought */
  iteration?: number;
  /** Enable typewriter effect for streaming content */
  enableTypewriter?: boolean;
  /** Typewriter speed in milliseconds per character */
  typewriterSpeed?: number;
  /** Custom className for the container */
  className?: string;
}

/**
 * ThoughtBubble - Displays AI thinking process with Markdown rendering
 * and optional typewriter animation effect.
 *
 * Features:
 * - Markdown rendering for rich thought content
 * - Typewriter effect for streaming updates
 * - Visual states for thinking vs completed
 * - Iteration display for multi-step reasoning
 */
export function ThoughtBubble({
  content,
  isStreaming = false,
  isComplete = false,
  iteration,
  enableTypewriter = true,
  typewriterSpeed = 15,
  className,
}: ThoughtBubbleProps) {
  // Typewriter effect state
  const [displayedContent, setDisplayedContent] = useState('');
  const [isTyping, setIsTyping] = useState(false);
  const previousContentRef = useRef('');
  const typewriterIntervalRef = useRef<number | null>(null);

  // Determine if we should animate
  const shouldAnimate = enableTypewriter && isStreaming;

  // Typewriter effect
  useEffect(() => {
    if (!shouldAnimate) {
      // No animation - show full content immediately
      setDisplayedContent(content);
      setIsTyping(false);
      return;
    }

    const previousContent = previousContentRef.current;
    const newContent = content;

    // If content was reset or shortened, reset displayed content
    if (newContent.length < previousContent.length) {
      setDisplayedContent('');
      previousContentRef.current = '';
    }

    // Find the new characters to type
    const startIndex = Math.min(displayedContent.length, newContent.length);
    const charsToType = newContent.slice(startIndex);

    if (charsToType.length > 0) {
      setIsTyping(true);
      let charIndex = 0;

      // Clear any existing interval
      if (typewriterIntervalRef.current) {
        clearInterval(typewriterIntervalRef.current);
      }

      typewriterIntervalRef.current = window.setInterval(() => {
        if (charIndex < charsToType.length) {
          setDisplayedContent((prev) => prev + charsToType[charIndex]);
          charIndex++;
        } else {
          // Finished typing
          if (typewriterIntervalRef.current) {
            clearInterval(typewriterIntervalRef.current);
            typewriterIntervalRef.current = null;
          }
          setIsTyping(false);
        }
      }, typewriterSpeed);
    }

    previousContentRef.current = newContent;

    // Cleanup interval on unmount
    return () => {
      if (typewriterIntervalRef.current) {
        clearInterval(typewriterIntervalRef.current);
      }
    };
  }, [content, shouldAnimate, typewriterSpeed, displayedContent.length]);

  // When not streaming anymore, show full content
  useEffect(() => {
    if (!isStreaming && content !== displayedContent) {
      setDisplayedContent(content);
    }
  }, [isStreaming, content, displayedContent]);

  // The content to render
  const renderContent = useMemo(() => {
    return shouldAnimate ? displayedContent : content;
  }, [shouldAnimate, displayedContent, content]);

  // Determine the current state
  const state = isComplete ? 'complete' : isStreaming || isTyping ? 'thinking' : 'idle';

  return (
    <div
      className={cn(
        'group flex gap-3 transition-all duration-200',
        className
      )}
    >
      {/* Icon */}
      <div
        className={cn(
          'flex-shrink-0 w-7 h-7 rounded-full flex items-center justify-center transition-colors duration-200',
          state === 'complete'
            ? 'bg-green-500/10'
            : 'bg-purple-500/10'
        )}
      >
        {state === 'thinking' ? (
          <Loader2 className="w-4 h-4 text-purple-500 animate-spin" />
        ) : state === 'complete' ? (
          <CheckCircle2 className="w-4 h-4 text-green-500" />
        ) : (
          <Brain className="w-4 h-4 text-purple-500" />
        )}
      </div>

      {/* Content */}
      <div className="flex-1 min-w-0">
        {/* Header */}
        <div className="flex items-center gap-2 mb-1.5">
          <span
            className={cn(
              'text-xs font-medium transition-colors duration-200',
              state === 'complete' ? 'text-green-500' : 'text-purple-500'
            )}
          >
            {state === 'thinking' ? 'Thinking...' : state === 'complete' ? 'Thought' : 'Thinking'}
          </span>
          {iteration !== undefined && (
            <span className="text-xs text-muted-foreground">
              Iteration {iteration}
            </span>
          )}
        </div>

        {/* Markdown Content */}
        <div
          className={cn(
            'prose prose-sm dark:prose-invert max-w-none',
            'prose-p:my-1 prose-p:leading-relaxed',
            'prose-headings:my-2 prose-headings:font-semibold',
            'prose-ul:my-1 prose-ol:my-1',
            'prose-li:my-0.5',
            'prose-code:bg-muted prose-code:px-1 prose-code:py-0.5 prose-code:rounded prose-code:text-xs',
            'prose-pre:bg-muted/50 prose-pre:p-3 prose-pre:rounded-lg',
            'text-foreground/80'
          )}
        >
          <ReactMarkdown>{renderContent}</ReactMarkdown>
          {/* Cursor for typewriter effect */}
          {isTyping && (
            <span className="inline-block w-0.5 h-4 bg-purple-500 animate-pulse ml-0.5 align-middle" />
          )}
        </div>
      </div>
    </div>
  );
}

export default ThoughtBubble;
