import { useEffect, useState, useCallback, useRef } from 'react';
import {
  Search,
  Download,
  Star,
  Loader2,
  ChevronDown,
  Check,
  AlertCircle,
  Shield,
  Award,
  ExternalLink,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@/lib/utils';
import { useClawHubStore } from '@/stores';
import { useSkillsStore } from '@/stores';
import type { ClawHubSkill } from '@/api/types';

const SORT_OPTIONS = [
  { value: 'trending', label: '热门' },
  { value: 'downloads', label: '下载量' },
  { value: 'stars', label: '星标' },
  { value: 'updated', label: '最新更新' },
];

export function ClawHubTab({ onInstalled }: { onInstalled?: () => void }) {
  const {
    skills,
    isLoading,
    isSearching,
    installingSlug,
    error,
    installError,
    hasMore,
    sortBy,
    searchQuery,
    browse,
    search,
    clearSearch,
    install,
    setSortBy,
    clearInstallError,
  } = useClawHubStore();

  const { skills: installedSkills, fetchSkills } = useSkillsStore();
  const [searchInput, setSearchInput] = useState('');
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Initial load
  useEffect(() => {
    if (skills.length === 0 && !isLoading && !isSearching) {
      browse(true);
    }
  }, []);

  // Debounced search
  const handleSearchInput = useCallback(
    (value: string) => {
      setSearchInput(value);
      if (debounceRef.current) clearTimeout(debounceRef.current);

      if (!value.trim()) {
        clearSearch();
        return;
      }

      debounceRef.current = setTimeout(() => {
        search(value);
      }, 300);
    },
    [search, clearSearch],
  );

  // Install handler
  const handleInstall = async (slug: string) => {
    clearInstallError();
    const success = await install(slug);
    if (success) {
      await fetchSkills();
      onInstalled?.();
    }
  };

  // Check if a skill is already installed
  const isInstalled = (slug: string) => {
    return installedSkills.some(
      (s) => s.name === slug || s.name.toLowerCase() === slug.toLowerCase(),
    );
  };

  const loading = isLoading || isSearching;

  return (
    <div className="flex flex-col flex-1 min-h-0">
      {/* Search + Sort bar */}
      <div className="px-8 pb-4 flex flex-col gap-3">
        <div className="flex gap-2">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
            <Input
              placeholder="搜索 ClawHub skills..."
              value={searchInput}
              onChange={(e) => handleSearchInput(e.target.value)}
              className="pl-9"
            />
          </div>

          {/* Sort dropdown */}
          <div className="relative">
            <select
              value={sortBy}
              onChange={(e) => setSortBy(e.target.value)}
              disabled={!!searchQuery}
              className={cn(
                'h-9 rounded-md border border-input bg-background px-3 pr-8 text-sm',
                'focus:outline-none focus:ring-1 focus:ring-ring',
                'disabled:opacity-50 disabled:cursor-not-allowed',
                'appearance-none cursor-pointer',
              )}
            >
              {SORT_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
            <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
          </div>
        </div>

        {installError && (
          <div className="flex items-center gap-2 text-xs text-destructive">
            <AlertCircle className="h-3.5 w-3.5 shrink-0" />
            {installError}
          </div>
        )}
      </div>

      {/* Skills grid */}
      <ScrollArea className="flex-1 overflow-hidden">
        <div className="px-8 pb-8">
        {error ? (
          <div className="flex flex-col items-center justify-center py-20 text-muted-foreground">
            <AlertCircle className="h-8 w-8 mb-2 opacity-50" />
            <p className="text-sm">{error}</p>
            <Button
              variant="outline"
              size="sm"
              className="mt-3"
              onClick={() => browse(true)}
            >
              重试
            </Button>
          </div>
        ) : loading && skills.length === 0 ? (
          <div className="flex items-center justify-center py-20 text-muted-foreground">
            <Loader2 className="h-5 w-5 animate-spin mr-2" />
            {isSearching ? '搜索中...' : '加载中...'}
          </div>
        ) : skills.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-20 text-muted-foreground">
            <Search className="h-8 w-8 mb-2 opacity-30" />
            <p className="text-sm">
              {searchQuery ? '没有找到匹配的 skills' : '暂无数据'}
            </p>
          </div>
        ) : (
          <>
            {searchQuery && (
              <div className="text-xs text-muted-foreground mb-4">
                搜索 &quot;{searchQuery}&quot; 找到 {skills.length} 个结果
              </div>
            )}

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {skills.map((skill) => (
                <ClawHubSkillCard
                  key={skill.slug}
                  skill={skill}
                  installed={isInstalled(skill.slug)}
                  installing={installingSlug === skill.slug}
                  onInstall={() => handleInstall(skill.slug)}
                />
              ))}
            </div>

            {/* Infinite scroll sentinel */}
            {hasMore && !searchQuery && (
              <LoadMoreSentinel loading={isLoading} onLoadMore={() => browse()} />
            )}
          </>
        )}
        </div>
      </ScrollArea>
    </div>
  );
}

function ClawHubSkillCard({
  skill,
  installed,
  installing,
  onInstall,
}: {
  skill: ClawHubSkill;
  installed: boolean;
  installing: boolean;
  onInstall: () => void;
}) {
  const name = skill.displayName || skill.slug;
  const isOfficial = !!skill.badges?.official;
  const isHighlighted = !!skill.badges?.highlighted;

  return (
    <div className="group rounded-xl border border-border/60 bg-card p-4 transition-all duration-200 hover:shadow-md hover:border-border">
      {/* Header */}
      <div className="flex items-start gap-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5">
            <h3 className="text-sm font-semibold truncate">{name}</h3>
            {isOfficial && (
              <Shield className="h-3.5 w-3.5 text-blue-500 shrink-0" />
            )}
            {isHighlighted && (
              <Award className="h-3.5 w-3.5 text-amber-500 shrink-0" />
            )}
          </div>
          {skill.owner && (
            <p className="text-[11px] text-muted-foreground mt-0.5">
              by {skill.owner.name || skill.owner.handle}
            </p>
          )}
          <p
            className="text-xs text-muted-foreground mt-1 line-clamp-2 cursor-default"
            title={skill.summary || ''}
          >
            {skill.summary || 'No description'}
          </p>
        </div>

        {/* Install button */}
        {installed ? (
          <span className="inline-flex items-center gap-1 text-[11px] text-emerald-600 dark:text-emerald-400 font-medium shrink-0 mt-0.5">
            <Check className="h-3.5 w-3.5" />
            已安装
          </span>
        ) : (
          <Button
            variant="outline"
            size="sm"
            onClick={onInstall}
            disabled={installing}
            className="shrink-0"
          >
            {installing ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <>
                <Download className="h-3.5 w-3.5 mr-1" />
                安装
              </>
            )}
          </Button>
        )}
      </div>

      {/* Footer stats */}
      <div className="mt-3 pt-3 border-t border-border/40 flex items-center gap-4">
        {skill.stats && (
          <>
            <div className="flex items-center gap-1">
              <Star className="h-3 w-3 text-muted-foreground" />
              <span className="text-[11px] text-muted-foreground font-medium">
                {formatCount(skill.stats.stars)}
              </span>
            </div>
            <div className="flex items-center gap-1">
              <Download className="h-3 w-3 text-muted-foreground" />
              <span className="text-[11px] text-muted-foreground font-medium">
                {formatCount(skill.stats.downloads)}
              </span>
            </div>
          </>
        )}
        {skill.latestVersion && (
          <span className="text-[11px] text-muted-foreground">
            v{skill.latestVersion.version}
          </span>
        )}
        <a
          href={`https://clawhub.ai/skill/${skill.slug}`}
          target="_blank"
          rel="noopener noreferrer"
          className="ml-auto flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground transition-colors"
        >
          <ExternalLink className="h-3 w-3" />
          ClawHub
        </a>
      </div>
    </div>
  );
}

function LoadMoreSentinel({
  loading,
  onLoadMore,
}: {
  loading: boolean;
  onLoadMore: () => void;
}) {
  const sentinelRef = useRef<HTMLDivElement>(null);
  const onLoadMoreRef = useRef(onLoadMore);
  onLoadMoreRef.current = onLoadMore;

  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;

    // Find the Radix ScrollArea viewport as the intersection root
    const viewport = el.closest('[data-slot="scroll-area-viewport"]');

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          onLoadMoreRef.current();
        }
      },
      { root: viewport, threshold: 0.1 },
    );

    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={sentinelRef} className="flex justify-center py-6">
      {loading && (
        <div className="flex items-center text-xs text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
          加载中...
        </div>
      )}
    </div>
  );
}

function formatCount(n: number): string {
  if (n >= 1000000) return `${(n / 1000000).toFixed(1)}M`;
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return n.toString();
}
