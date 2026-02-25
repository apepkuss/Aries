import { useEffect, useState } from 'react';
import {
  Blocks,
  Wrench,
  RefreshCw,
  Download,
  X,
  ChevronDown,
  Key,
  Loader2,
} from 'lucide-react';
import { Switch } from '@/components/ui/switch';
import { cn } from '@/lib/utils';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog';
import { useSkillsStore } from '@/stores';
import { getSkillEnv, updateSkillEnv } from '@/api/skills';
import { EnvVarsForm } from './EnvVarsForm';
import { ClawHubTab } from './ClawHubTab';
import type { SkillSummary } from '@/api/types';

type TabId = 'installed' | 'clawhub';

export function SkillsPage() {
  const {
    skills,
    isLoading,
    isInstalling,
    installError,
    fetchSkills,
    installSkill,
    clearInstallError,
  } = useSkillsStore();

  const [activeTab, setActiveTab] = useState<TabId>('installed');
  const [showInstallForm, setShowInstallForm] = useState(false);
  const [installUrl, setInstallUrl] = useState('');
  const [installName, setInstallName] = useState('');
  const [installEnvVars, setInstallEnvVars] = useState<Record<string, string>>({});
  const [showEnvSection, setShowEnvSection] = useState(false);

  useEffect(() => {
    fetchSkills();
  }, [fetchSkills]);

  const handleInstall = async () => {
    if (!installUrl.trim()) return;
    const envVars = Object.keys(installEnvVars).length > 0 ? installEnvVars : undefined;
    const success = await installSkill(installUrl.trim(), installName.trim() || undefined, envVars);
    if (success) {
      setShowInstallForm(false);
      setInstallUrl('');
      setInstallName('');
      setInstallEnvVars({});
      setShowEnvSection(false);
    }
  };

  const handleCancelInstall = () => {
    setShowInstallForm(false);
    setInstallUrl('');
    setInstallName('');
    setInstallEnvVars({});
    setShowEnvSection(false);
    clearInstallError();
  };

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="px-8 pt-8 pb-4">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-bold tracking-tight">Skills</h1>
            <p className="text-base text-muted-foreground mt-1">
              Give Moss superpowers.
            </p>
          </div>
          <div className="flex items-center gap-2">
            {activeTab === 'installed' && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    if (showInstallForm) {
                      handleCancelInstall();
                    } else {
                      setShowInstallForm(true);
                    }
                  }}
                  disabled={isInstalling}
                >
                  {showInstallForm ? (
                    <>
                      <X className="h-3.5 w-3.5 mr-1.5" />
                      取消
                    </>
                  ) : (
                    <>
                      <Download className="h-3.5 w-3.5 mr-1.5" />
                      安装
                    </>
                  )}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={fetchSkills}
                  disabled={isLoading}
                >
                  <RefreshCw className={`h-3.5 w-3.5 mr-1.5 ${isLoading ? 'animate-spin' : ''}`} />
                  刷新
                </Button>
              </>
            )}
          </div>
        </div>

        {/* Tabs */}
        <div className="mt-4 flex gap-1 border-b border-border">
          <button
            type="button"
            className={cn(
              'px-4 py-2 text-sm font-medium transition-colors relative',
              activeTab === 'installed'
                ? 'text-foreground'
                : 'text-muted-foreground hover:text-foreground',
            )}
            onClick={() => setActiveTab('installed')}
          >
            <div className="flex items-center gap-1.5">
              <Blocks className="h-3.5 w-3.5" />
              已安装
              {skills.length > 0 && (
                <span className="text-[11px] text-muted-foreground">
                  ({skills.length})
                </span>
              )}
            </div>
            {activeTab === 'installed' && (
              <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-primary" />
            )}
          </button>
          <button
            type="button"
            className={cn(
              'px-4 py-2 text-sm font-medium transition-colors relative',
              activeTab === 'clawhub'
                ? 'text-foreground'
                : 'text-muted-foreground hover:text-foreground',
            )}
            onClick={() => setActiveTab('clawhub')}
          >
            <div className="flex items-center gap-1.5">
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" className="h-3.5 w-3.5" aria-label="ClawHub">
                <g fill="#ff4f40">
                  <rect x="5" y="3" width="6" height="1"/><rect x="4" y="4" width="8" height="1"/>
                  <rect x="3" y="5" width="10" height="1"/><rect x="3" y="6" width="10" height="1"/>
                  <rect x="3" y="7" width="10" height="1"/><rect x="4" y="8" width="8" height="1"/>
                  <rect x="5" y="9" width="6" height="1"/><rect x="5" y="12" width="6" height="1"/>
                  <rect x="6" y="13" width="4" height="1"/>
                </g>
                <g fill="#ff775f">
                  <rect x="1" y="6" width="2" height="1"/><rect x="2" y="5" width="1" height="1"/>
                  <rect x="2" y="7" width="1" height="1"/><rect x="13" y="6" width="2" height="1"/>
                  <rect x="13" y="5" width="1" height="1"/><rect x="13" y="7" width="1" height="1"/>
                </g>
                <g fill="#081016">
                  <rect x="6" y="5" width="1" height="1"/><rect x="9" y="5" width="1" height="1"/>
                </g>
                <g fill="#f5fbff">
                  <rect x="6" y="4" width="1" height="1"/><rect x="9" y="4" width="1" height="1"/>
                </g>
              </svg>
              ClawHub
            </div>
            {activeTab === 'clawhub' && (
              <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-primary" />
            )}
          </button>
        </div>

        {/* Install Form (only on installed tab) */}
        {activeTab === 'installed' && showInstallForm && (
          <div className="mt-4 p-4 rounded-lg border border-border bg-muted/30">
            <div className="flex flex-col gap-3">
              <div className="flex gap-2">
                <Input
                  placeholder="https://example.com/skill.tar.gz"
                  value={installUrl}
                  onChange={(e) => setInstallUrl(e.target.value)}
                  disabled={isInstalling}
                  className="flex-1"
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') handleInstall();
                  }}
                />
                <Input
                  placeholder="名称（可选）"
                  value={installName}
                  onChange={(e) => setInstallName(e.target.value)}
                  disabled={isInstalling}
                  className="w-40"
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') handleInstall();
                  }}
                />
                <Button
                  size="sm"
                  onClick={handleInstall}
                  disabled={isInstalling || !installUrl.trim()}
                >
                  {isInstalling ? (
                    <>
                      <RefreshCw className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                      安装中...
                    </>
                  ) : (
                    '安装'
                  )}
                </Button>
              </div>

              {/* Env Vars Section */}
              <div>
                <button
                  type="button"
                  className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1"
                  onClick={() => setShowEnvSection(!showEnvSection)}
                >
                  <Key className="h-3 w-3" />
                  环境变量（可选）
                  <ChevronDown
                    className={`h-3 w-3 transition-transform ${showEnvSection ? 'rotate-180' : ''}`}
                  />
                </button>
                {showEnvSection && (
                  <div className="mt-2">
                    <EnvVarsForm
                      envVars={installEnvVars}
                      onChange={setInstallEnvVars}
                      disabled={isInstalling}
                    />
                  </div>
                )}
              </div>

              {installError && (
                <p className="text-xs text-destructive">{installError}</p>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Tab Content */}
      {activeTab === 'installed' ? (
        <ScrollArea className="flex-1 px-8 pb-8">
          {isLoading && skills.length === 0 ? (
            <div className="flex items-center justify-center py-20 text-muted-foreground">
              <RefreshCw className="h-5 w-5 animate-spin mr-2" />
              加载中...
            </div>
          ) : skills.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-20 text-muted-foreground">
              <Blocks className="h-12 w-12 mb-3 opacity-30" />
              <p className="text-sm">暂无已加载的 Skills</p>
              <Button
                variant="link"
                size="sm"
                className="mt-2"
                onClick={() => setActiveTab('clawhub')}
              >
                从 ClawHub 浏览和安装
              </Button>
            </div>
          ) : (
            <>
              <div className="text-xs text-foreground mb-4 font-semibold">
                已安装 {skills.length} 个 Skills
              </div>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {skills.map((skill) => (
                  <SkillCard key={skill.name} skill={skill} />
                ))}
              </div>
            </>
          )}
        </ScrollArea>
      ) : (
        <ClawHubTab
          onInstalled={() => {
            fetchSkills();
            setActiveTab('installed');
          }}
        />
      )}
    </div>
  );
}

function SkillCard({ skill }: { skill: SkillSummary }) {
  const toggleSkill = useSkillsStore((s) => s.toggleSkill);
  const [envDialogOpen, setEnvDialogOpen] = useState(false);
  const [envVars, setEnvVars] = useState<Record<string, string>>({});
  const [isLoadingEnv, setIsLoadingEnv] = useState(false);
  const [isSavingEnv, setIsSavingEnv] = useState(false);
  const [envDirty, setEnvDirty] = useState(false);
  const [envError, setEnvError] = useState<string | null>(null);
  const [envConfigured, setEnvConfigured] = useState(false);

  const handleOpenEnvDialog = async (open: boolean) => {
    if (open) {
      setIsLoadingEnv(true);
      setEnvError(null);
      try {
        const response = await getSkillEnv(skill.name);
        setEnvVars(response.env_vars);
        setEnvConfigured(Object.keys(response.env_vars).length > 0);
      } catch {
        setEnvError('Failed to load environment variables');
      }
      setIsLoadingEnv(false);
      setEnvDirty(false);
    }
    setEnvDialogOpen(open);
  };

  const handleEnvChange = (newEnvVars: Record<string, string>) => {
    setEnvVars(newEnvVars);
    setEnvDirty(true);
  };

  const handleSaveEnv = async () => {
    setIsSavingEnv(true);
    setEnvError(null);
    try {
      await updateSkillEnv(skill.name, envVars);
      setEnvDirty(false);
      setEnvConfigured(Object.keys(envVars).some((k) => k.trim()));
      setEnvDialogOpen(false);
    } catch {
      setEnvError('Failed to save environment variables');
    }
    setIsSavingEnv(false);
  };

  return (
    <div className={cn(
      "group rounded-xl border border-border/60 bg-card p-4 transition-all duration-200 hover:shadow-md hover:border-border",
      !skill.enabled && "opacity-50"
    )}>
      {/* Skill header */}
      <div className="flex items-start gap-3">
        <div className="shrink-0 h-9 w-9 rounded-lg bg-primary/10 flex items-center justify-center">
          <Blocks className="h-4.5 w-4.5 text-primary" />
        </div>
        <div className="flex-1 min-w-0">
          <h3 className="text-sm font-semibold truncate">{skill.name}</h3>
          <p
            className="text-xs text-muted-foreground mt-0.5 line-clamp-2 cursor-default"
            title={skill.description || 'No description'}
          >
            {skill.description || 'No description'}
          </p>
        </div>
        <Switch
          checked={skill.enabled}
          onCheckedChange={(checked) => toggleSkill(skill.name, checked)}
        />
      </div>

      {/* Footer info */}
      <div className="mt-3 pt-3 border-t border-border/40 flex items-center gap-3">
        {/* Tools count */}
        <div className="flex items-center gap-1">
          <Wrench className="h-3 w-3 text-muted-foreground" />
          <span className="text-[11px] text-muted-foreground font-medium">
            {skill.allowed_tools.length} tools
          </span>
        </div>

        {/* Env vars status + dialog */}
        <Dialog open={envDialogOpen} onOpenChange={handleOpenEnvDialog}>
          <DialogTrigger asChild>
            <button className="flex items-center gap-1 text-[11px] font-medium hover:text-foreground transition-colors cursor-pointer">
              <Key className="h-3 w-3" />
              <span className={envConfigured ? 'text-emerald-600 dark:text-emerald-400' : 'text-amber-600 dark:text-amber-400'}>
                {envConfigured ? '环境变量已配置' : '配置环境变量'}
              </span>
            </button>
          </DialogTrigger>
          <DialogContent className="sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>环境变量 - {skill.name}</DialogTitle>
              <DialogDescription>
                为此 Skill 配置环境变量，这些变量将在 Skill 脚本执行时注入。
              </DialogDescription>
            </DialogHeader>
            <div className="py-2">
              {isLoadingEnv ? (
                <div className="flex items-center text-xs text-muted-foreground py-4">
                  <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" />
                  加载中...
                </div>
              ) : (
                <EnvVarsForm
                  envVars={envVars}
                  onChange={handleEnvChange}
                  disabled={isSavingEnv}
                />
              )}
              {envError && (
                <p className="text-xs text-destructive mt-2">{envError}</p>
              )}
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setEnvDialogOpen(false)}>
                取消
              </Button>
              <Button onClick={handleSaveEnv} disabled={isSavingEnv || !envDirty}>
                {isSavingEnv && <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />}
                保存
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
    </div>
  );
}

export default SkillsPage;
