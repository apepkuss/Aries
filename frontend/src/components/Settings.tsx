import { useState, useEffect } from "react";
import { X, Save, Loader2, Eye, EyeOff } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAriesStore } from "../store";

interface SettingsProps {
  isOpen: boolean;
  onClose: () => void;
}

export function Settings({ isOpen, onClose }: SettingsProps) {
  const { chatConfig, fetchChatConfig, updateChatConfig, isConfigLoading } =
    useAriesStore();

  const [url, setUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [saveStatus, setSaveStatus] = useState<"idle" | "success" | "error">(
    "idle"
  );

  useEffect(() => {
    if (isOpen) {
      fetchChatConfig();
    }
  }, [isOpen, fetchChatConfig]);

  useEffect(() => {
    if (chatConfig) {
      setUrl(chatConfig.url);
      setApiKey(chatConfig.api_key);
      setModel(chatConfig.model);
    }
  }, [chatConfig]);

  const handleSave = async () => {
    // Validate required fields
    if (!model.trim()) {
      setSaveStatus("error");
      return;
    }

    setIsSaving(true);
    setSaveStatus("idle");

    try {
      await updateChatConfig({ url, api_key: apiKey, model });
      setSaveStatus("success");
      setTimeout(() => setSaveStatus("idle"), 2000);
    } catch (error) {
      console.error("Failed to save config:", error);
      setSaveStatus("error");
    } finally {
      setIsSaving(false);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/50 backdrop-blur-sm"
        onClick={onClose}
      />

      {/* Modal */}
      <div className="relative bg-card border border-border rounded-2xl shadow-2xl w-full max-w-md mx-4 overflow-hidden animate-in fade-in zoom-in-95 duration-200">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-border">
          <h2 className="text-lg font-semibold text-foreground">Settings</h2>
          <button
            onClick={onClose}
            className="p-2 rounded-lg hover:bg-muted transition-colors"
          >
            <X className="w-4 h-4 text-muted-foreground" />
          </button>
        </div>

        {/* Content */}
        <div className="p-6 space-y-6">
          {isConfigLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
            </div>
          ) : (
            <>
              {/* Chat Section */}
              <div className="space-y-4">
                <h3 className="text-sm font-medium text-muted-foreground uppercase tracking-wider">
                  Chat Configuration
                </h3>

                {/* API URL */}
                <div className="space-y-2">
                  <label
                    htmlFor="api-url"
                    className="text-sm font-medium text-foreground"
                  >
                    API URL
                  </label>
                  <input
                    id="api-url"
                    type="url"
                    value={url}
                    onChange={(e) => setUrl(e.target.value)}
                    placeholder="https://api.openai.com/v1"
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                    className="w-full px-4 py-2.5 bg-muted/50 border border-border rounded-xl text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/50 transition-all"
                  />
                </div>

                {/* API Key */}
                <div className="space-y-2">
                  <label
                    htmlFor="api-key"
                    className="text-sm font-medium text-foreground"
                  >
                    API Key
                  </label>
                  <div className="relative">
                    <input
                      id="api-key"
                      type={showApiKey ? "text" : "password"}
                      value={apiKey}
                      onChange={(e) => setApiKey(e.target.value)}
                      placeholder="sk-..."
                      autoCapitalize="none"
                      autoCorrect="off"
                      spellCheck={false}
                      className="w-full px-4 py-2.5 pr-12 bg-muted/50 border border-border rounded-xl text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/50 transition-all"
                    />
                    <button
                      type="button"
                      onClick={() => setShowApiKey(!showApiKey)}
                      className="absolute right-3 top-1/2 -translate-y-1/2 p-1 rounded hover:bg-muted transition-colors"
                    >
                      {showApiKey ? (
                        <EyeOff className="w-4 h-4 text-muted-foreground" />
                      ) : (
                        <Eye className="w-4 h-4 text-muted-foreground" />
                      )}
                    </button>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    Leave empty to use environment variable:
                    DEFAULT_CHAT_SERVICE_API_KEY
                  </p>
                </div>

                {/* Model */}
                <div className="space-y-2">
                  <label
                    htmlFor="model"
                    className="text-sm font-medium text-foreground"
                  >
                    Model <span className="text-red-500">*</span>
                  </label>
                  <input
                    id="model"
                    type="text"
                    value={model}
                    onChange={(e) => setModel(e.target.value)}
                    placeholder="gpt-4o-mini"
                    autoCapitalize="none"
                    autoCorrect="off"
                    spellCheck={false}
                    className={cn(
                      "w-full px-4 py-2.5 bg-muted/50 border rounded-xl text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/50 transition-all",
                      !model.trim() && saveStatus === "error"
                        ? "border-red-500"
                        : "border-border"
                    )}
                  />
                  <p className="text-xs text-muted-foreground">
                    The model name to use for chat completions (e.g.,
                    gpt-4o-mini, gpt-4o, claude-3-opus)
                  </p>
                </div>
              </div>

            </>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 px-6 py-4 border-t border-border bg-muted/30">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={isSaving || isConfigLoading}
            className={cn(
              "flex items-center justify-center gap-2 px-4 py-2 text-sm font-medium rounded-lg transition-colors min-w-[80px]",
              saveStatus === "success"
                ? "bg-green-500/20 text-green-500"
                : saveStatus === "error"
                  ? "bg-red-500/20 text-red-500"
                  : "bg-primary text-primary-foreground hover:bg-primary/90",
              (isSaving || isConfigLoading) && "opacity-50 cursor-not-allowed"
            )}
          >
            {isSaving ? (
              <>
                <Loader2 className="w-4 h-4 animate-spin" />
                Saving...
              </>
            ) : saveStatus === "success" ? (
              "Saved!"
            ) : saveStatus === "error" ? (
              "Error"
            ) : (
              <>
                <Save className="w-4 h-4" />
                Save
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
