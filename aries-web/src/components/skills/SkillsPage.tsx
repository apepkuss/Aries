import { useEffect, useState } from 'react';
import { Blocks, Wrench, RefreshCw, Download, X } from 'lucide-react';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useSkillsStore } from '@/stores';
import type { SkillSummary } from '@/api/types';

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

  const [showInstallForm, setShowInstallForm] = useState(false);
  const [installUrl, setInstallUrl] = useState('');
  const [installName, setInstallName] = useState('');

  useEffect(() => {
    fetchSkills();
  }, [fetchSkills]);

  const handleInstall = async () => {
    if (!installUrl.trim()) return;
    const success = await installSkill(installUrl.trim(), installName.trim() || undefined);
    if (success) {
      setShowInstallForm(false);
      setInstallUrl('');
      setInstallName('');
    }
  };

  const handleCancelInstall = () => {
    setShowInstallForm(false);
    setInstallUrl('');
    setInstallName('');
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
              Give Aries superpowers.
            </p>
          </div>
          <div className="flex items-center gap-2">
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
          </div>
        </div>

        {/* Install Form */}
        {showInstallForm && (
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
              {installError && (
                <p className="text-xs text-destructive">{installError}</p>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Content */}
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
    </div>
  );
}

function SkillCard({ skill }: { skill: SkillSummary }) {
  return (
    <div className="group rounded-xl border border-border/60 bg-card p-4 transition-all duration-200 hover:shadow-md hover:border-border">
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
      </div>

      {/* Tools */}
      {skill.allowed_tools.length > 0 && (
        <div className="mt-3 pt-3 border-t border-border/40">
          <div className="flex items-center gap-1 mb-2">
            <Wrench className="h-3 w-3 text-muted-foreground" />
            <span className="text-[11px] text-muted-foreground font-medium">
              Tools ({skill.allowed_tools.length})
            </span>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {skill.allowed_tools.map((tool) => (
              <Badge
                key={tool}
                variant="secondary"
                className="text-[10px] px-1.5 py-0 h-5 font-mono"
              >
                {tool}
              </Badge>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export default SkillsPage;
