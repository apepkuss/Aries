import { useEffect, useRef, useState } from "react";
import { useAriesStore } from "./store";
import { Send, Zap, Command, Sparkles, Settings as SettingsIcon, Layers } from "lucide-react";
import { cn } from "@/lib/utils";
import { Settings } from "./components/Settings";
import { ExecutionPanel } from "./components/ExecutionPanel";
import { useExecutionEvents } from "./hooks/useExecutionEvents";

function App() {
  const { messages, isLoading, isConfigured, fetchServerInfo, sendMessage, sendMessageWithPlan, fetchChatConfig } = useAriesStore();
  const [input, setInput] = useState("");
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [usePlanMode, setUsePlanMode] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Subscribe to execution events when in Plan mode
  const executionState = useExecutionEvents();

  useEffect(() => {
    fetchServerInfo();
    fetchChatConfig();
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, executionState.steps]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (input.trim() && !isLoading) {
      const message = input.trim();
      setInput("");

      if (usePlanMode) {
        // Use Plan mode with execution transparency
        await sendMessageWithPlan(message);
      } else {
        // Use simple chat mode
        await sendMessage(message);
      }
    }
  };

  const hasMessages = messages.length > 0;
  const showExecutionPanel = executionState.isExecuting || executionState.steps.length > 0;

  return (
    <div className="flex flex-col h-screen bg-background text-foreground font-sans selection:bg-accent overflow-hidden relative transition-colors duration-300">

      {/* Background Decorative Gradient - Adjusted for Light/Dark */}
      <div className="absolute top-0 left-1/2 -translate-x-1/2 w-full h-[500px] bg-gradient-to-b from-muted/40 to-transparent pointer-events-none" />

      {/* Header - Minimal */}
      <header className="absolute top-0 left-0 w-full p-6 flex justify-between items-center z-20">
        <div className="flex items-center gap-2 opacity-50 text-foreground pointer-events-none">
          <Zap className="w-4 h-4" />
          <span className="text-xs font-semibold tracking-widest uppercase">Aries</span>
        </div>
        <div className="flex items-center gap-3">
          {!hasMessages && (
            <div className="flex items-center gap-1.5 px-2 py-1 rounded bg-muted/50 border border-border text-[10px] text-muted-foreground font-mono opacity-60 pointer-events-none">
               <Command className="w-3 h-3" />
               <span>SPACE TO TOGGLE</span>
            </div>
          )}
          <button
            onClick={() => setIsSettingsOpen(true)}
            className="p-2 rounded-lg hover:bg-muted/50 transition-colors opacity-50 hover:opacity-100"
            title="Settings"
          >
            <SettingsIcon className="w-4 h-4 text-foreground" />
          </button>
        </div>
      </header>

      {/* Messages Area */}
      <div
        ref={scrollRef}
        className={cn(
          "flex-1 w-full max-w-2xl mx-auto overflow-y-auto px-6 py-6 space-y-8 scrollbar-hide transition-all duration-700 ease-out z-10",
          hasMessages ? "opacity-100 pt-20 pb-32" : "opacity-0 hidden"
        )}
      >
        {messages.map((msg, index) => {
          const isLatestUserMessage = msg.role === 'user' && index === messages.length - 1;
          const showActivePanel = isLatestUserMessage && showExecutionPanel;
          const hasHistoricalSteps = msg.role === 'assistant' && msg.executionSteps && msg.executionSteps.length > 0;

          return (
            <div key={msg.id} className="space-y-6">
              {/* Historical Execution Details for Assistant Message */}
              {hasHistoricalSteps && (
                <div className="w-full opacity-80 scale-[0.98] origin-top">
                  <ExecutionPanel
                    steps={msg.executionSteps!}
                    phase={null}
                    statusMessage={null}
                    subtaskProgress={null}
                    isExecuting={false}
                  />
                </div>
              )}

              <div
                className={cn(
                  "flex flex-col gap-2 max-w-[95%] animate-in fade-in slide-in-from-bottom-2 duration-500",
                  msg.role === 'user' ? "ml-auto items-end" : "mr-auto items-start"
                )}
              >
                <div className={cn(
                  "px-5 py-3 rounded-2xl text-[15px] leading-relaxed shadow-sm backdrop-blur-sm",
                  msg.role === 'user'
                    ? "bg-primary text-primary-foreground font-medium"
                    : "bg-muted/60 border border-border text-foreground"
                )}>
                  {msg.content}
                </div>
                {msg.role === 'assistant' && (
                  <div className="flex items-center gap-2 opacity-30 text-[10px] pl-2 uppercase tracking-wider text-muted-foreground">
                    <Sparkles className="w-3 h-3" /> Aries
                  </div>
                )}
              </div>

              {/* Active Execution Panel - shows during Plan mode execution, appearing after the active user message */}
              {showActivePanel && (
                <div className="w-full animate-in fade-in slide-in-from-bottom-2 duration-500 py-2">
                  <ExecutionPanel
                    steps={executionState.steps}
                    phase={executionState.phase}
                    statusMessage={executionState.statusMessage}
                    subtaskProgress={executionState.subtaskProgress}
                    isExecuting={executionState.isExecuting}
                  />
                </div>
              )}
            </div>
          );
        })}

        {isLoading && !showExecutionPanel && (
          <div className="flex items-center gap-3 text-muted-foreground text-sm pl-4 animate-pulse">
             <div className="w-2 h-2 bg-muted-foreground/50 rounded-full animate-bounce [animation-delay:-0.3s]" />
             <div className="w-2 h-2 bg-muted-foreground/50 rounded-full animate-bounce [animation-delay:-0.15s]" />
             <div className="w-2 h-2 bg-muted-foreground/50 rounded-full animate-bounce" />
          </div>
        )}
      </div>

      {/* Input Area */}
      <div className={cn(
        "absolute w-full px-6 transition-all duration-700 cubic-bezier(0.16, 1, 0.3, 1) z-50 flex flex-col items-center",
        hasMessages ? "bottom-8" : "top-1/2 -translate-y-1/2"
      )}>
        <div className="w-full max-w-[640px] relative group">

           {/* Ambient Glow */}
           <div className={cn(
             "absolute -inset-0.5 bg-gradient-to-r from-accent/50 to-accent/50 rounded-2xl opacity-0 group-hover:opacity-100 transition duration-700 blur-xl group-focus-within:opacity-100",
             isLoading && "opacity-50 animate-pulse"
           )}></div>

           <form
            onSubmit={handleSubmit}
            className={cn(
              "relative bg-card border border-border rounded-xl shadow-2xl overflow-hidden transition-all duration-300",
              !hasMessages ? "p-1" : "p-0 rounded-2xl"
            )}
          >
            <div className="flex items-center px-4 py-3 gap-3">
               {!hasMessages && <Zap className={cn("w-5 h-5 transition-colors duration-500", isConfigured ? "text-muted-foreground" : "text-amber-500/50")} />}
               <input
                ref={inputRef}
                autoFocus
                value={input}
                onChange={(e) => setInput(e.target.value)}
                placeholder={hasMessages ? "Reply..." : "What would you like to do?"}
                className={cn(
                  "flex-1 bg-transparent border-none outline-none text-foreground placeholder:text-muted-foreground font-medium",
                  hasMessages ? "text-base h-7" : "text-xl h-10"
                )}
              />
              <div className="flex items-center gap-2">
                 {/* Plan Mode Toggle */}
                 <button
                   type="button"
                   onClick={() => setUsePlanMode(!usePlanMode)}
                   className={cn(
                     "p-2 rounded-lg transition-all flex items-center justify-center",
                     usePlanMode
                       ? "bg-primary/10 text-primary"
                       : "text-muted-foreground hover:text-foreground hover:bg-muted"
                   )}
                   title={usePlanMode ? "Plan Mode (On)" : "Plan Mode (Off)"}
                 >
                   <Layers className="w-4 h-4" />
                 </button>
                 {!input.trim() && !hasMessages && (
                   <div className="hidden sm:flex items-center gap-1 px-2 py-1 rounded bg-muted text-[10px] text-muted-foreground font-bold tracking-widest">
                    AI
                   </div>
                 )}
                 <button
                  type="submit"
                  disabled={!input.trim() || isLoading}
                  className={cn(
                    "p-2 rounded-lg transition-all decoration-0 flex items-center justify-center",
                    input.trim()
                      ? "bg-primary text-primary-foreground hover:bg-primary/90"
                      : "opacity-0"
                  )}
                >
                  <Send className="w-4 h-4 fill-current" />
                </button>
              </div>
            </div>
            {!hasMessages && isConfigured && (
              <div className="h-[1px] w-full bg-border opacity-50" />
            )}
             {!hasMessages && (
               <div className="px-4 py-3 bg-muted/20 flex items-center justify-between text-[11px] text-muted-foreground font-medium">
                  <div className="flex gap-4">
                    <span
                      className={cn(
                        "cursor-pointer transition-colors",
                        !usePlanMode ? "text-foreground" : "hover:text-foreground"
                      )}
                      onClick={() => setUsePlanMode(false)}
                    >
                      Chat
                    </span>
                    <span
                      className={cn(
                        "cursor-pointer transition-colors",
                        usePlanMode ? "text-foreground" : "hover:text-foreground"
                      )}
                      onClick={() => setUsePlanMode(true)}
                    >
                      Plan
                    </span>
                    <span className="hover:text-foreground cursor-pointer transition-colors">Recent</span>
                  </div>
                  <div className="flex gap-2 opacity-50">
                    Press <span className="font-mono bg-muted px-1 rounded">↵</span> to submit
                  </div>
               </div>
             )}
          </form>
        </div>
      </div>

      {/* Settings Modal */}
      <Settings isOpen={isSettingsOpen} onClose={() => setIsSettingsOpen(false)} />
    </div>
  );
}

export default App;
