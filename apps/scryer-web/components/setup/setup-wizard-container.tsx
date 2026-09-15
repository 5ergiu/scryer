import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { toast } from "sonner";
import { useClient } from "urql";

import {
  browsePathQuery,
  qualityProfilesInitQuery,
  setupStatusQuery,
  setupWizardProviderTypesInitQuery,
} from "@/lib/graphql/queries";
import {
  saveQualityProfileSettingsMutation,
  updateLibraryPathsMutation,
  completeSetupMutation,
} from "@/lib/graphql/mutations";
import { buildDownloadClientTypeOptions } from "@/lib/utils/download-clients";
import { useDownloadClientSetup } from "@/lib/hooks/use-download-client-setup";
import {
  useIndexerSetup,
  type SetupIndexerProviderOption,
} from "@/lib/hooks/use-indexer-setup";
import { usePluginManagement } from "@/lib/hooks/use-plugin-management";
import { localPathStyleFromRuntimeValue } from "@/lib/utils/local-path-style";
import {
  runAdvisorySetupMediaPathSave,
  type InvalidSetupMediaPathFields,
  type SetupMediaPathField,
} from "@/lib/utils/setup-media-paths";
import {
  qualityProfileSettingsToEntries,
  qualityProfileEntryToMutationInput,
} from "@/lib/utils/quality-profiles";
import type {
  FacetQualityPrefs,
  QualityTargetId,
  ViewCategoryId,
} from "@/lib/types/quality-profiles";
import type { ProviderTypeInfo } from "@/lib/types";

import ScryerLogo from "@/components/scryer-logo";
import { cn } from "@/lib/utils";
import { SetupProgressBar } from "./setup-progress-bar";
import { SetupWelcomeView } from "./setup-welcome-view";
import { SetupPersonaView } from "./setup-persona-view";
import { SetupMediaPathsView } from "./setup-media-paths-view";
import { SetupDownloadClientView } from "./setup-download-client-view";
import { SetupIndexerView } from "./setup-indexer-view";
import { SetupSummaryView } from "./setup-summary-view";
import SetupImportWizard from "./setup-import-wizard";
import { SetupPluginsView } from "./setup-plugins-view";
import { SetupRestoreView } from "./setup-restore-view";
import { SetupIntroMark, prefersReducedMotion } from "./setup-intro";

const FALLBACK_PROVIDER_OPTIONS: SetupIndexerProviderOption[] = [];

interface SetupWizardContainerProps {
  t: (
    key: string,
    values?: Record<string, string | number | boolean | null | undefined>,
  ) => string;
  isReentry?: boolean;
  onBackendRestarting: () => void;
}

function formatQualityTarget(target: QualityTargetId): string {
  switch (target) {
    case "8k":
      return "8K";
    case "4k":
      return "4K";
    case "1080p":
      return "1080P";
  }
  return target;
}

export function SetupWizardContainer({
  t,
  isReentry,
  onBackendRestarting,
}: SetupWizardContainerProps) {
  const client = useClient();
  const navigate = useNavigate();

  const tImport = useCallback(
    (key: string, values?: Record<string, unknown>) =>
      t(
        key,
        values as
          | Record<string, string | number | boolean | null | undefined>
          | undefined,
      ),
    [t],
  );

  // ── Wizard path + step (URL-driven for browser back/forward) ──────
  const [searchParams, setSearchParams] = useSearchParams();
  const wizardPath: "fresh" | "import" | "restore" =
    searchParams.get("path") === "import"
      ? "import"
      : searchParams.get("path") === "restore"
        ? "restore"
        : "fresh";
  const currentStep = parseInt(searchParams.get("step") || "0", 10);
  const [canRestoreSetup, setCanRestoreSetup] = useState(false);

  // The welcome plays once, when setup first opens — not when it is reopened
  // from Settings, and not on moving between steps.
  const [intro, setIntro] = useState<"waiting" | "playing" | null>(() =>
    isReentry || prefersReducedMotion() ? null : "waiting",
  );
  const headerLogoRef = useRef<HTMLDivElement>(null);
  const startIntro = useCallback(() => setIntro("playing"), []);
  const finishIntro = useCallback(() => setIntro(null), []);
  const [restoreAvailabilityChecked, setRestoreAvailabilityChecked] =
    useState(false);

  const goToStep = useCallback(
    (step: number, path?: "fresh" | "import" | "restore") => {
      const p = path ?? wizardPath;
      if (step === 0) {
        setSearchParams({});
      } else {
        setSearchParams({ path: p, step: String(step) });
      }
    },
    [wizardPath, setSearchParams],
  );

  // Widened adapter so the import wizard's `(step, path?: string)` signature
  // satisfies the container's narrower path union (it only ever passes
  // "import" or undefined).
  const goToImportStep = useCallback(
    (step: number, path?: string) =>
      goToStep(step, path as "fresh" | "import" | "restore" | undefined),
    [goToStep],
  );

  useEffect(() => {
    let cancelled = false;
    setCanRestoreSetup(false);
    setRestoreAvailabilityChecked(false);

    client
      .query(setupStatusQuery, {}, { requestPolicy: "network-only" })
      .toPromise()
      .then(({ data }) => {
        if (cancelled) return;
        setCanRestoreSetup(data?.setupStatus?.setupComplete === false);
      })
      .catch(() => {
        if (cancelled) return;
        setCanRestoreSetup(false);
      })
      .finally(() => {
        if (cancelled) return;
        setRestoreAvailabilityChecked(true);
      });

    return () => {
      cancelled = true;
    };
  }, [client]);

  useEffect(() => {
    if (wizardPath !== "restore" || !restoreAvailabilityChecked || canRestoreSetup) {
      return;
    }

    const nextSearchParams = new URLSearchParams();
    if (isReentry) {
      nextSearchParams.set("reentry", "1");
    }
    setSearchParams(nextSearchParams, { replace: true });
  }, [
    canRestoreSetup,
    isReentry,
    restoreAvailabilityChecked,
    setSearchParams,
    wizardPath,
  ]);

  // ── Step 1 (fresh) / Step 3 (import): Quality Preferences ─────────
  const [facetPrefs, setFacetPrefs] = useState<
    Record<ViewCategoryId, FacetQualityPrefs>
  >({
    MOVIE: { quality: "1080p", persona: "BALANCED" },
    SERIES: { quality: "1080p", persona: "BALANCED" },
    ANIME: { quality: "1080p", persona: "BALANCED" },
  });
  const [personaSaving, setPersonaSaving] = useState(false);

  // ── Step 2 (fresh): Media Paths ─────────────────────────────────────
  const [moviesPath, setMoviesPath] = useState("/data/movies");
  const [seriesPath, setSeriesPath] = useState("/data/series");
  const [animePath, setAnimePath] = useState("/data/anime");
  const [mediaPathsSaving, setMediaPathsSaving] = useState(false);
  const [mediaPathsError, setMediaPathsError] = useState<string | null>(null);
  const [invalidMediaPathFields, setInvalidMediaPathFields] =
    useState<InvalidSetupMediaPathFields>({});
  const [mediaPathValidationUnavailable, setMediaPathValidationUnavailable] =
    useState(false);

  // ── Step 4 (fresh): Download Client ─────────────────────────────────
  const {
    dcDraft,
    dcLocalPathStyle,
    setDcLocalPathStyle,
    setDcTypeOptions,
    availableDcTypeOptions,
    selectedDcConfigFields,
    dcTesting,
    dcTestResult,
    dcSaving,
    dcSaved,
    dcError,
    handleDcDraftChange,
    testDownloadClient,
    handleDcTestAndSave,
  } = useDownloadClientSetup({ client });

  // ── Step 5 (fresh): Indexer ─────────────────────────────────────────
  const {
    idxName,
    idxProviderType,
    idxConfigValues,
    idxProviderOptions,
    setIdxProviderOptions,
    idxTesting,
    idxTestResult,
    idxSaving,
    idxSaved,
    idxError,
    handleIdxNameChange,
    handleIdxProviderTypeChange,
    handleIdxConfigValueChange,
    testIndexer,
    handleIdxTestAndSave,
  } = useIndexerSetup({ client, t });

  // ── Summary / Finish (fresh path) ───────────────────────────────────
  const [finishingAction, setFinishingAction] = useState<"finish" | null>(null);
  const finishing = finishingAction !== null;

  const refreshProviderOptions = useCallback(async () => {
    try {
      const { data, error } = await client
        .query(setupWizardProviderTypesInitQuery, {})
        .toPromise();
      if (
        error &&
        !data?.downloadClientProviderTypes &&
        !data?.indexerProviderTypes
      )
        throw error;

      setDcLocalPathStyle(
        localPathStyleFromRuntimeValue(data?.runtimeInfo?.runtimePathStyle),
      );
      setDcTypeOptions(
        buildDownloadClientTypeOptions(
          (data?.downloadClientProviderTypes as
            | ProviderTypeInfo[]
            | undefined) ?? [],
        ),
      );

      if (data?.indexerProviderTypes?.length) {
        setIdxProviderOptions(
          data.indexerProviderTypes.map((provider: ProviderTypeInfo) => ({
            value: provider.providerType,
            label: provider.name,
            defaultBaseUrl: provider.defaultBaseUrl || undefined,
            configFields: provider.configFields ?? [],
          })),
        );
      } else {
        setIdxProviderOptions(FALLBACK_PROVIDER_OPTIONS);
      }
    } catch {
      setDcTypeOptions(buildDownloadClientTypeOptions([]));
      setIdxProviderOptions(FALLBACK_PROVIDER_OPTIONS);
    }
  }, [
    client,
    setDcLocalPathStyle,
    setDcTypeOptions,
    setIdxProviderOptions,
  ]);

  const {
    plugins,
    pluginsLoading,
    pluginsRefreshing,
    mutatingPluginIds,
    pluginProgress,
    pluginErrors,
    pluginsError,
    refreshPluginsRegistry,
    installPlugin,
    uninstallPlugin,
  } = usePluginManagement({ client, t, refreshProviderOptions });

  // ── Step labels per path ────────────────────────────────────────────
  const stepLabels =
    wizardPath === "import"
      ? [
          t("setup.stepConnect"),
          t("setup.stepLibraries"),
          t("setup.stepQuality"),
          t("setup.stepSources"),
          t("setup.stepSummary"),
        ]
      : wizardPath === "restore"
        ? [t("setup.stepRestore")]
        : [
            t("setup.stepPersona"),
            t("setup.stepMediaPaths"),
            t("setup.stepPlugins"),
            t("setup.stepDownloadClient"),
            t("setup.stepIndexer"),
            t("setup.stepSummary"),
          ];

  // ── Quality preferences save (per-facet) ────────────────────────────
  const saveFacetQualityPrefs = useCallback(
    async (nextStep: number) => {
      setPersonaSaving(true);
      try {
        const { data } = await client
          .query(qualityProfilesInitQuery, {})
          .toPromise();
        const existingProfiles = qualityProfileSettingsToEntries(
          data?.qualityProfileSettings,
        );

        // Build per-facet profiles from templates
        const WIZARD_FACETS: { facet: ViewCategoryId; name: string }[] = [
          { facet: "MOVIE", name: "Movies" },
          { facet: "SERIES", name: "Series" },
          { facet: "ANIME", name: "Anime" },
        ];
        const wizardProfileIds = WIZARD_FACETS.map((f) => `wizard-${f.facet}`);
        const builtinProfileIds = ["8k", "4k", "1080p"];
        const keptProfiles = existingProfiles.filter(
          (p) =>
            !wizardProfileIds.includes(p.id) &&
            !builtinProfileIds.includes(p.id),
        );

        for (const { facet, name } of WIZARD_FACETS) {
          const prefs = facetPrefs[facet];
          const template = existingProfiles.find((p) => p.id === prefs.quality);
          if (template) {
            const profileName = `${name} (${formatQualityTarget(prefs.quality)})`;
            keptProfiles.push({
              id: `wizard-${facet}`,
              name: profileName,
              criteria: { ...template.criteria },
            });
          }
        }

        await client
          .mutation(saveQualityProfileSettingsMutation, {
            input: {
              profiles: keptProfiles.map(qualityProfileEntryToMutationInput),
              globalProfileId: null,
              globalScoringPersona: null,
              categorySelections: WIZARD_FACETS.map(({ facet }) => ({
                scope: facet,
                profileId: `wizard-${facet}`,
                inheritGlobal: false,
              })),
              categoryPersonaSelections: WIZARD_FACETS.map(({ facet }) => ({
                scope: facet,
                persona: facetPrefs[facet].persona,
                inheritGlobal: false,
              })),
              replaceExisting: true,
            },
          })
          .toPromise();

        goToStep(nextStep);
      } catch (err) {
        console.warn("Failed to save quality preferences, continuing", err);
        goToStep(nextStep);
      } finally {
        setPersonaSaving(false);
      }
    },
    [client, facetPrefs, goToStep],
  );

  // ── Media paths save ────────────────────────────────────────────────
  const clearInvalidMediaPathField = useCallback(
    (field: SetupMediaPathField) => {
      setMediaPathValidationUnavailable(false);
      setInvalidMediaPathFields((current) => {
        if (current[field] !== true) {
          return current;
        }
        const next = { ...current };
        delete next[field];
        return next;
      });
    },
    [],
  );

  const handleMoviesPathChange = useCallback(
    (value: string) => {
      setMoviesPath(value);
      clearInvalidMediaPathField("movies");
    },
    [clearInvalidMediaPathField],
  );

  const handleSeriesPathChange = useCallback(
    (value: string) => {
      setSeriesPath(value);
      clearInvalidMediaPathField("series");
    },
    [clearInvalidMediaPathField],
  );

  const handleAnimePathChange = useCallback(
    (value: string) => {
      setAnimePath(value);
      clearInvalidMediaPathField("anime");
    },
    [clearInvalidMediaPathField],
  );

  const saveMediaPaths = useCallback(async () => {
    setMediaPathsSaving(true);
    setMediaPathsError(null);
    try {
      const trimmedMovies = moviesPath.trim();
      const trimmedSeries = seriesPath.trim();
      const trimmedAnime = animePath.trim();
      if (!trimmedMovies && !trimmedSeries && !trimmedAnime) {
        setInvalidMediaPathFields({});
        setMediaPathValidationUnavailable(false);
        goToStep(3);
        return;
      }
      await runAdvisorySetupMediaPathSave({
        input: {
          moviePath: trimmedMovies,
          seriesPath: trimmedSeries,
          animePath: trimmedAnime.length > 0 ? trimmedAnime : null,
        },
        validatePath: async (path) => {
          const { error } = await client
            .query(
              browsePathQuery,
              { path },
              { requestPolicy: "network-only" },
            )
            .toPromise();
          return error;
        },
        onValidation: ({ invalidPathFields, unavailable }) => {
          setInvalidMediaPathFields(invalidPathFields);
          setMediaPathValidationUnavailable(unavailable);
        },
        savePaths: async (input) => {
          const { error } = await client
            .mutation(updateLibraryPathsMutation, { input })
            .toPromise();
          if (error) throw error;
        },
        onSaved: ({ invalidPathFields, unavailable }) => {
          if (Object.values(invalidPathFields).some(Boolean)) {
            toast.warning(t("setup.mediaPathsNotReachableWarning"));
          } else if (unavailable) {
            toast.warning(t("setup.mediaPathsVerificationUnavailable"));
          }
          goToStep(3);
        },
      });
    } catch (err) {
      setMediaPathsError(err instanceof Error ? err.message : "Failed to save");
    } finally {
      setMediaPathsSaving(false);
    }
  }, [
    animePath,
    client,
    goToStep,
    moviesPath,
    seriesPath,
    t,
  ]);

  // ── Complete setup ──────────────────────────────────────────────────
  const navigateAfterSetup = useCallback(() => {
    navigate(isReentry ? "/settings" : "/movies", { replace: true });
  }, [isReentry, navigate]);

  const finishSetup = useCallback(async () => {
    setFinishingAction("finish");
    try {
      const { data, error } = await client
        .mutation(completeSetupMutation, {})
        .toPromise();
      if (error) {
        throw error;
      }
      if (!data?.completeSetup?.completed) {
        throw new Error(t("setup.connectError"));
      }
      navigateAfterSetup();
    } catch {
      navigateAfterSetup();
    } finally {
      setFinishingAction(null);
    }
  }, [client, navigateAfterSetup, t]);

  // ── Render ──────────────────────────────────────────────────────────

  // Step mapping for progress bar (step 0 = welcome, not shown in bar)
  const progressStep = currentStep > 0 ? currentStep - 1 : -1;
  const isWideImportStep =
    currentStep === 0 ||
    (wizardPath === "import" && (currentStep === 1 || currentStep === 2));
  // Quality (3) and Sources (4) are dense, multi-column tables — the narrow
  // shell crushes the library identity column. Give them a comfortable
  // mid-width so names and facet pills aren't truncated.
  const isMediumImportStep =
    wizardPath === "import" && (currentStep === 3 || currentStep === 4);
  const isPersonaStep = wizardPath === "fresh" && currentStep === 1;
  const isPluginsStep = wizardPath === "fresh" && currentStep === 3;
  const shellMaxWidth = isWideImportStep
    ? "max-w-6xl"
    : isMediumImportStep
      ? "max-w-4xl"
      : isPluginsStep
        ? "max-w-6xl"
      : isPersonaStep
        ? "max-w-3xl"
        : "max-w-2xl";

  return (
    <div
      className={cn(
        "mx-auto flex min-h-screen w-full flex-col items-center justify-center px-4 py-10",
        shellMaxWidth,
        intro && "setup-intro",
        intro === "playing" && "setup-intro-playing",
      )}
    >
      <div className="setup-intro-header mb-8 flex items-center gap-2.5">
        {wizardPath !== "import" ? (
          <div ref={headerLogoRef} className="setup-intro-logo">
            <ScryerLogo className="h-20 w-20" />
          </div>
        ) : null}
        {currentStep > 0 ? (
          <span className="setup-intro-wordmark font-[var(--font-space-grotesk)] text-lg font-bold tracking-tight text-[var(--scry-ink2)]">
            Scryer
          </span>
        ) : null}
      </div>
      {intro ? (
        <SetupIntroMark
          targetRef={headerLogoRef}
          ready={restoreAvailabilityChecked}
          onStart={startIntro}
          onDone={finishIntro}
        />
      ) : null}

      {currentStep > 0 && (
        <div className="mb-8 w-full">
          <SetupProgressBar
            currentStep={progressStep}
            stepLabels={stepLabels}
            onStepClick={(i) => goToStep(i + 1, wizardPath)}
          />
        </div>
      )}

      {/* ── Step 0: Welcome (shared) ─────────────────────────────────── */}
      {currentStep === 0 && (
        <SetupWelcomeView
          t={t}
          onFreshSetup={() => goToStep(1, "fresh")}
          onImportSetup={() => goToStep(1, "import")}
          onRestoreSetup={() => goToStep(1, "restore")}
          onSkip={finishSetup}
          skipping={finishing}
          canRestoreSetup={canRestoreSetup}
        />
      )}

      {/* ════════════════════════════════════════════════════════════════ */}
      {/* FRESH PATH                                                      */}
      {/* ════════════════════════════════════════════════════════════════ */}

      {currentStep === 1 && wizardPath === "fresh" && (
        <SetupPersonaView
          t={t}
          facetPrefs={facetPrefs}
          onFacetPrefsChange={(facet, prefs) =>
            setFacetPrefs((prev) => ({ ...prev, [facet]: prefs }))
          }
          onNext={() => saveFacetQualityPrefs(2)}
          onBack={() => goToStep(0)}
          onSkip={() => goToStep(2)}
          saving={personaSaving}
        />
      )}

      {currentStep === 2 && wizardPath === "fresh" && (
        <SetupMediaPathsView
          t={t}
          moviesPath={moviesPath}
          seriesPath={seriesPath}
          animePath={animePath}
          onMoviesPathChange={handleMoviesPathChange}
          onSeriesPathChange={handleSeriesPathChange}
          onAnimePathChange={handleAnimePathChange}
          onNext={saveMediaPaths}
          onBack={() => goToStep(1)}
          onSkip={() => goToStep(3)}
          saving={mediaPathsSaving}
          error={mediaPathsError}
          invalidPathFields={invalidMediaPathFields}
          validationUnavailable={mediaPathValidationUnavailable}
        />
      )}

      {currentStep === 3 && wizardPath === "fresh" && (
        <SetupPluginsView
          t={t}
          plugins={plugins}
          loading={pluginsLoading}
          refreshing={pluginsRefreshing}
          mutatingPluginIds={mutatingPluginIds}
          pluginProgress={pluginProgress}
          pluginErrors={pluginErrors}
          error={pluginsError}
          onRefreshRegistry={refreshPluginsRegistry}
          onInstallPlugin={installPlugin}
          onUninstallPlugin={uninstallPlugin}
          onNext={() => goToStep(4)}
          onBack={() => goToStep(2)}
        />
      )}

      {currentStep === 4 && wizardPath === "fresh" && (
        <SetupDownloadClientView
          t={t}
          draft={dcDraft}
          downloadClientTypeOptions={availableDcTypeOptions}
          configFields={selectedDcConfigFields}
          localPathStyle={dcLocalPathStyle}
          onDraftChange={handleDcDraftChange}
          onTestConnection={dcSaved ? testDownloadClient : handleDcTestAndSave}
          onNext={() => goToStep(5)}
          onBack={() => goToStep(3)}
          onSkip={() => goToStep(5)}
          testing={dcTesting}
          testResult={dcTestResult}
          saving={dcSaving}
          saved={dcSaved}
          error={dcError}
        />
      )}

      {currentStep === 5 && wizardPath === "fresh" && (
        <SetupIndexerView
          t={t}
          name={idxName}
          providerType={idxProviderType}
          configValues={idxConfigValues}
          providerOptions={idxProviderOptions}
          onNameChange={handleIdxNameChange}
          onProviderTypeChange={handleIdxProviderTypeChange}
          onConfigValueChange={handleIdxConfigValueChange}
          onTestConnection={idxSaved ? testIndexer : handleIdxTestAndSave}
          onNext={() => goToStep(6)}
          onBack={() => goToStep(4)}
          onSkip={() => goToStep(6)}
          testing={idxTesting}
          testResult={idxTestResult}
          saving={idxSaving}
          saved={idxSaved}
          error={idxError}
        />
      )}

      {currentStep === 6 && wizardPath === "fresh" && (
        <SetupSummaryView
          t={t}
          facetPrefs={facetPrefs}
          moviesPaths={[moviesPath]}
          seriesPaths={[seriesPath]}
          animePaths={animePath ? [animePath] : []}
          downloadClientName={dcDraft.name || dcDraft.clientType}
          indexerName={idxName || idxProviderType}
          onFinish={finishSetup}
          onBack={() => goToStep(5)}
          finishing={finishing}
          finishingAction={finishingAction}
        />
      )}

      {/* ════════════════════════════════════════════════════════════════ */}
      {/* IMPORT PATH                                                     */}
      {/* ════════════════════════════════════════════════════════════════ */}

      {currentStep === 1 && wizardPath === "restore" && canRestoreSetup && (
        <SetupRestoreView
          t={t}
          onBack={() => goToStep(0)}
          onBackendRestarting={onBackendRestarting}
        />
      )}

      {wizardPath === "import" && currentStep >= 1 && (
        <SetupImportWizard
          client={client}
          t={tImport}
          currentStep={currentStep}
          goToStep={goToImportStep}
          onExit={navigateAfterSetup}
        />
      )}
    </div>
  );
}
