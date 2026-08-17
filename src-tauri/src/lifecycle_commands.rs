use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use p2p_domain::{AmountMode, ExactDecimal, UserIntent};
use p2p_lifecycle::{
    ActionableFailure, EmptyKind, FailureKind, LifecycleController, LifecycleError,
    LifecycleStatus, LifecycleView, MarketContextDraft, PersistedLifecycle, PreparedAcquisition,
    RefreshSettings, RefreshStage, RefreshTrigger, SETTINGS_KEY, SETTINGS_SECTION,
    prepare_acquisition_for_publication, validate_acquisition_context, validate_acquisition_pair,
};
use p2p_persistence::{CatalogPairInput, PublicationInput, RetentionPolicy, SummaryInput};
use p2p_provider::{AcquisitionEligibility, AcquisitionRequest, CircuitState, ProviderError};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::contracts::{AppErrorCategory, AppErrorEnvelope};
use crate::{LifecycleRuntimeState, PersistenceRuntimeState, ProviderRuntimeState};

const PROGRESS_EVENT: &str = "lifecycle-acquisition-progress";

#[derive(Clone, Copy)]
enum DueRequirement {
    None,
    Normal,
    Retry,
    Wake,
}

pub fn initialize_lifecycle(
    store: &p2p_persistence::PersistenceStore,
    now_ms: i64,
) -> Result<LifecycleRuntimeState, p2p_persistence::PersistenceError> {
    let mut controller = LifecycleController::loading();
    match store.load_setting(SETTINGS_SECTION, SETTINGS_KEY)? {
        None => {
            let first_run = PersistedLifecycle::default();
            first_run.validate().map_err(|error| {
                p2p_persistence::PersistenceError::InvalidInput(format!(
                    "compiled first-run lifecycle defaults are invalid: {error}"
                ))
            })?;
            store.save_setting(
                SETTINGS_SECTION,
                SETTINGS_KEY,
                &serde_json::to_value(&first_run)?,
                now_ms,
            )?;
            controller
                .restore(first_run)
                .expect("compiled first-run lifecycle defaults are valid");
            controller.ready();
        }
        Some(value) => match serde_json::from_value::<PersistedLifecycle>(value) {
            Ok(restored) => {
                if controller.restore(restored).is_ok() {
                    controller.ready();
                }
            }
            Err(error) => controller.finish_error(ActionableFailure::invalid_restored(format!(
                "Saved lifecycle JSON is invalid: {error}"
            ))),
        },
    }
    if let Err(error) = store.prune(now_ms, RetentionPolicy::default()) {
        controller.record_maintenance_warning(format!(
            "Startup retention maintenance failed without changing lifecycle restoration: {error}"
        ));
    }
    Ok(LifecycleRuntimeState {
        controller: Arc::new(Mutex::new(controller)),
        active_cancellation: Arc::new(Mutex::new(None)),
    })
}

pub fn start_auto_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut startup_attempted = false;
        loop {
            ticker.tick().await;
            let Ok(now) = now_ms() else {
                continue;
            };
            let lifecycle = app.state::<LifecycleRuntimeState>();
            let provider = app.state::<ProviderRuntimeState>();
            let persistence = app.state::<PersistenceRuntimeState>();
            let view = lifecycle.controller.lock().await.view(now);
            let (normal_due, retry_due) = {
                let controller = lifecycle.controller.lock().await;
                (controller.due(now), controller.retry_due(now))
            };
            let provider_retry = retry_due
                && matches!(
                    view.status,
                    LifecycleStatus::Error {
                        failure: ActionableFailure {
                            kind: FailureKind::Provider,
                            ..
                        }
                    }
                )
                && provider.0.circuit_state().await == CircuitState::Closed;
            let empty_retry = retry_due && matches!(view.status, LifecycleStatus::Empty { .. });
            if !normal_due && !provider_retry && !empty_retry {
                continue;
            }
            let trigger = if !startup_attempted && view.last_success_ms.is_none() {
                RefreshTrigger::Startup
            } else {
                RefreshTrigger::Automatic
            };
            startup_attempted = true;
            let due_requirement = if normal_due {
                DueRequirement::Normal
            } else {
                DueRequirement::Retry
            };
            let _ = run_refresh(
                &app,
                &lifecycle,
                &provider,
                &persistence,
                trigger,
                None,
                due_requirement,
            )
            .await;
        }
    });
}

#[tauri::command]
pub async fn get_lifecycle_view(
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    Ok(lifecycle.controller.lock().await.view(now_ms()?))
}

#[tauri::command]
pub async fn reset_lifecycle_state(
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
    persistence: tauri::State<'_, PersistenceRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    let active = lifecycle.active_cancellation.lock().await;
    if active.is_some() {
        return Err(command_error(
            "LIFECYCLE-RESET-BUSY",
            AppErrorCategory::Lifecycle,
            "Cancel the active refresh before resetting lifecycle settings.",
            true,
            None,
        ));
    }
    let now = now_ms()?;
    let reset = PersistedLifecycle::default();
    reset.validate().map_err(configuration_error)?;
    let mut controller = lifecycle.controller.lock().await;
    let was_offline = controller.view(now).offline;
    let mut replacement = LifecycleController::loading();
    replacement
        .restore(reset.clone())
        .map_err(configuration_error)?;
    replacement.ready();
    replacement.set_offline(was_offline);
    save_lifecycle(&persistence.0, &reset, now)?;
    *controller = replacement;
    Ok(controller.view(now))
}

#[tauri::command]
pub async fn update_market_draft(
    draft: MarketContextDraft,
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
    persistence: tauri::State<'_, PersistenceRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    let active = lifecycle.active_cancellation.lock().await;
    if active.is_some() {
        return Err(command_error(
            "LIFECYCLE-EDIT-BUSY",
            AppErrorCategory::Lifecycle,
            "Wait for the active refresh before changing the market draft.",
            true,
            None,
        ));
    }
    draft.validate().map_err(configuration_error)?;
    let now = now_ms()?;
    let mut controller = lifecycle.controller.lock().await;
    let mut replacement = controller.clone();
    replacement
        .update_draft(draft)
        .map_err(configuration_error)?;
    let candidate = replacement.persisted();
    candidate.validate().map_err(configuration_error)?;
    save_lifecycle(&persistence.0, &candidate, now)?;
    *controller = replacement;
    Ok(controller.view(now))
}

#[tauri::command]
pub async fn update_refresh_settings(
    settings: RefreshSettings,
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
    persistence: tauri::State<'_, PersistenceRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    let active = lifecycle.active_cancellation.lock().await;
    if active.is_some() {
        return Err(command_error(
            "LIFECYCLE-SETTINGS-BUSY",
            AppErrorCategory::Lifecycle,
            "Wait for the active refresh before changing refresh settings.",
            true,
            None,
        ));
    }
    settings.validate().map_err(configuration_error)?;
    let now = now_ms()?;
    let mut controller = lifecycle.controller.lock().await;
    let mut replacement = controller.clone();
    replacement
        .update_settings(settings)
        .map_err(configuration_error)?;
    let candidate = replacement.persisted();
    candidate.validate().map_err(configuration_error)?;
    save_lifecycle(&persistence.0, &candidate, now)?;
    *controller = replacement;
    Ok(controller.view(now))
}

#[tauri::command]
pub async fn apply_market_context(
    app: AppHandle,
    draft: MarketContextDraft,
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
    provider: tauri::State<'_, ProviderRuntimeState>,
    persistence: tauri::State<'_, PersistenceRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    run_refresh(
        &app,
        &lifecycle,
        &provider,
        &persistence,
        RefreshTrigger::Apply,
        Some(draft),
        DueRequirement::None,
    )
    .await
}

#[tauri::command]
pub async fn refresh_market(
    app: AppHandle,
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
    provider: tauri::State<'_, ProviderRuntimeState>,
    persistence: tauri::State<'_, PersistenceRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    run_refresh(
        &app,
        &lifecycle,
        &provider,
        &persistence,
        RefreshTrigger::Manual,
        None,
        DueRequirement::None,
    )
    .await
}

#[tauri::command]
pub async fn refresh_if_due(
    app: AppHandle,
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
    provider: tauri::State<'_, ProviderRuntimeState>,
    persistence: tauri::State<'_, PersistenceRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    run_refresh(
        &app,
        &lifecycle,
        &provider,
        &persistence,
        RefreshTrigger::Automatic,
        None,
        DueRequirement::Normal,
    )
    .await
}

#[tauri::command]
pub async fn refresh_after_wake(
    app: AppHandle,
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
    provider: tauri::State<'_, ProviderRuntimeState>,
    persistence: tauri::State<'_, PersistenceRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    run_refresh(
        &app,
        &lifecycle,
        &provider,
        &persistence,
        RefreshTrigger::Wake,
        None,
        DueRequirement::Wake,
    )
    .await
}

#[tauri::command]
pub async fn set_offline(
    offline: bool,
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    let active = lifecycle.active_cancellation.lock().await;
    if offline && let Some(cancellation) = active.as_ref() {
        cancellation.cancel();
    }
    let now = now_ms()?;
    let mut controller = lifecycle.controller.lock().await;
    controller.set_offline(offline);
    Ok(controller.view(now))
}

#[tauri::command]
pub async fn cancel_refresh(
    lifecycle: tauri::State<'_, LifecycleRuntimeState>,
) -> Result<LifecycleView, AppErrorEnvelope> {
    cancel_active(&lifecycle).await;
    Ok(lifecycle.controller.lock().await.view(now_ms()?))
}

async fn cancel_active(lifecycle: &LifecycleRuntimeState) {
    if let Some(cancellation) = lifecycle.active_cancellation.lock().await.as_ref() {
        cancellation.cancel();
    }
}

async fn run_refresh(
    app: &AppHandle,
    lifecycle: &LifecycleRuntimeState,
    provider: &ProviderRuntimeState,
    persistence: &PersistenceRuntimeState,
    trigger: RefreshTrigger,
    apply: Option<MarketContextDraft>,
    due_requirement: DueRequirement,
) -> Result<LifecycleView, AppErrorEnvelope> {
    let request_started_ms = now_ms()?;
    let mut active = lifecycle.active_cancellation.lock().await;
    if active.is_some() {
        return Err(command_error(
            "LIFECYCLE-REFRESH-BUSY",
            AppErrorCategory::Lifecycle,
            "A refresh is already in progress.",
            true,
            None,
        ));
    }
    let cancellation = CancellationToken::new();
    let applying = apply.is_some();
    let (request_id, context, settings) = {
        let mut controller = lifecycle.controller.lock().await;
        let due = match due_requirement {
            DueRequirement::None => true,
            DueRequirement::Normal => controller.due(request_started_ms),
            DueRequirement::Retry => controller.retry_due(request_started_ms),
            DueRequirement::Wake => controller.due_after_wake(request_started_ms),
        };
        if !due {
            return Ok(controller.view(request_started_ms));
        }

        let mut replacement = controller.clone();
        if let Some(draft) = apply {
            draft.validate().map_err(configuration_error)?;
            replacement
                .update_draft(draft)
                .and_then(|_| replacement.apply_draft())
                .map_err(configuration_error)?;
        }
        let context = replacement
            .applied()
            .validate()
            .map_err(configuration_error)?;
        let settings = replacement.settings();
        let request_id = provider.0.next_request_id();
        replacement
            .begin_refresh(&request_id, trigger, request_started_ms)
            .map_err(lifecycle_error)?;
        if applying {
            let candidate = replacement.persisted();
            candidate.validate().map_err(configuration_error)?;
            save_lifecycle(&persistence.0, &candidate, request_started_ms)?;
        }
        *controller = replacement;
        (request_id, context, settings)
    };
    *active = Some(cancellation.clone());
    drop(active);

    let provider_request = AcquisitionRequest {
        request_id: request_id.clone(),
        pair: context.pair.clone(),
        transaction_amount: match context.amount.mode() {
            AmountMode::Fiat => Some(context.amount.value()),
            AmountMode::Asset => None,
        },
        selected_payment_methods: context.filters.selected_payments().clone(),
        payment_logic: context.filters.payment_logic(),
        target: context.target,
        local_eligibility: Some(AcquisitionEligibility {
            amount: context.amount,
            filters: context.filters.clone(),
        }),
    };
    if let Err(view) = advance_refresh(lifecycle, RefreshStage::Acquiring, request_started_ms).await
    {
        clear_active(lifecycle).await;
        return Ok(view);
    }
    let progress_app = app.clone();
    let acquisition = provider
        .0
        .acquire(provider_request, cancellation.clone(), move |progress| {
            let _ = progress_app.emit(PROGRESS_EVENT, progress);
        })
        .await;

    let acquisition = match acquisition {
        Ok(value) => value,
        Err(ProviderError::Cancelled) => {
            lifecycle.controller.lock().await.finish_cancelled();
            clear_active(lifecycle).await;
            return Ok(lifecycle.controller.lock().await.view(now_ms()?));
        }
        Err(error) => {
            lifecycle
                .controller
                .lock()
                .await
                .finish_error(provider_failure(&error));
            clear_active(lifecycle).await;
            return Ok(lifecycle.controller.lock().await.view(now_ms()?));
        }
    };
    if cancellation.is_cancelled() {
        lifecycle.controller.lock().await.finish_cancelled();
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(now_ms()?));
    }

    if let Err(view) =
        advance_refresh(lifecycle, RefreshStage::Validating, request_started_ms).await
    {
        clear_active(lifecycle).await;
        return Ok(view);
    }
    if let Err(error) = validate_acquisition_pair(&acquisition, &context) {
        lifecycle
            .controller
            .lock()
            .await
            .finish_error(validation_failure(&error));
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(now_ms()?));
    }
    let catalog_verified_ms = match now_ms() {
        Ok(value) => value,
        Err(error) => {
            lifecycle
                .controller
                .lock()
                .await
                .finish_error(clock_failure(error.message));
            clear_active(lifecycle).await;
            return Ok(lifecycle.controller.lock().await.view(request_started_ms));
        }
    };
    if catalog_verified_ms < request_started_ms
        || acquisition
            .page_receipts
            .iter()
            .any(|receipt| receipt.received_ms() > catalog_verified_ms)
    {
        lifecycle
            .controller
            .lock()
            .await
            .finish_error(clock_failure(
                "The system clock moved backwards before catalog validation.",
            ));
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(catalog_verified_ms));
    }
    let payment_methods = acquisition
        .buy
        .ads
        .iter()
        .chain(&acquisition.sell.ads)
        .flat_map(|normalized| normalized.ad.payments().iter().cloned())
        .collect();
    if cancellation.is_cancelled() {
        lifecycle.controller.lock().await.finish_cancelled();
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(catalog_verified_ms));
    }
    if let Err(error) = persistence.0.save_catalog_pair(CatalogPairInput {
        pair: acquisition.pair.clone(),
        enabled: true,
        disabled_reason: None,
        verified_at_ms: catalog_verified_ms,
        disabled_at_ms: None,
        precision: serde_json::json!({
            "status": "provider-unspecified",
            "storage": "exact-decimal-text"
        }),
        payment_methods,
    }) {
        lifecycle
            .controller
            .lock()
            .await
            .finish_error(persistence_failure(&error));
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(catalog_verified_ms));
    }
    if cancellation.is_cancelled() {
        lifecycle.controller.lock().await.finish_cancelled();
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(catalog_verified_ms));
    }
    if let Err(view) =
        advance_refresh(lifecycle, RefreshStage::Calculating, catalog_verified_ms).await
    {
        clear_active(lifecycle).await;
        return Ok(view);
    }
    let acquisition = match prepare_acquisition_for_publication(acquisition, &context) {
        Ok(PreparedAcquisition::Publish(value)) => value,
        Ok(PreparedAcquisition::Empty(kind)) => {
            lifecycle.controller.lock().await.finish_empty(
                kind,
                match kind {
                    EmptyKind::ProviderEmpty => {
                        "The provider confirmed that both market sides are empty."
                    }
                    EmptyKind::NoMatchingResults => {
                        "No complete two-side result matches the applied filters."
                    }
                },
            );
            clear_active(lifecycle).await;
            return Ok(lifecycle.controller.lock().await.view(now_ms()?));
        }
        Err(error) => {
            lifecycle
                .controller
                .lock()
                .await
                .finish_error(calculation_failure(&error));
            clear_active(lifecycle).await;
            return Ok(lifecycle.controller.lock().await.view(now_ms()?));
        }
    };

    if let Err(error) = validate_acquisition_context(&acquisition, &context) {
        lifecycle
            .controller
            .lock()
            .await
            .finish_error(validation_failure(&error));
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(now_ms()?));
    }
    if cancellation.is_cancelled() {
        lifecycle.controller.lock().await.finish_cancelled();
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(now_ms()?));
    }
    if let Err(view) =
        advance_refresh(lifecycle, RefreshStage::Committing, catalog_verified_ms).await
    {
        clear_active(lifecycle).await;
        return Ok(view);
    }
    let Some(last_page_received_ms) = acquisition
        .page_receipts
        .iter()
        .map(|receipt| receipt.received_ms())
        .max()
    else {
        lifecycle
            .controller
            .lock()
            .await
            .finish_error(clock_failure(
                "A non-empty acquisition has no page receipt timestamps.",
            ));
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(request_started_ms));
    };
    let validated_ms = match now_ms() {
        Ok(value) => value,
        Err(error) => {
            lifecycle
                .controller
                .lock()
                .await
                .finish_error(clock_failure(error.message));
            clear_active(lifecycle).await;
            return Ok(lifecycle.controller.lock().await.view(request_started_ms));
        }
    };
    if validated_ms < request_started_ms || validated_ms < last_page_received_ms {
        lifecycle
            .controller
            .lock()
            .await
            .finish_error(clock_failure(
                "The system clock moved backwards during refresh validation.",
            ));
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(validated_ms));
    }
    let committed_ms = match now_ms() {
        Ok(value) if value >= validated_ms => value,
        Ok(_) => {
            lifecycle
                .controller
                .lock()
                .await
                .finish_error(clock_failure(
                    "The system clock moved backwards before atomic publication.",
                ));
            clear_active(lifecycle).await;
            return Ok(lifecycle.controller.lock().await.view(validated_ms));
        }
        Err(error) => {
            lifecycle
                .controller
                .lock()
                .await
                .finish_error(clock_failure(error.message));
            clear_active(lifecycle).await;
            return Ok(lifecycle.controller.lock().await.view(validated_ms));
        }
    };
    let summaries = vec![
        SummaryInput {
            intent: UserIntent::BuyAsset,
            metric_key: "eligible-count".to_owned(),
            value: Some(ExactDecimal::from_u64(u64::from(
                acquisition.buy.quality.valid(),
            ))),
            unit: "ads".to_owned(),
        },
        SummaryInput {
            intent: UserIntent::SellAsset,
            metric_key: "eligible-count".to_owned(),
            value: Some(ExactDecimal::from_u64(u64::from(
                acquisition.sell.quality.valid(),
            ))),
            unit: "ads".to_owned(),
        },
    ];
    if cancellation.is_cancelled() {
        lifecycle.controller.lock().await.finish_cancelled();
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(now_ms()?));
    }
    let publication = persistence.0.publish_complete_snapshot(PublicationInput {
        acquisition: &acquisition,
        context: context.persistence_context(),
        request_started_ms,
        last_page_received_ms,
        validated_ms,
        committed_ms,
        agent_checked_ms: None,
        refresh_interval_seconds: settings.interval_seconds,
        summaries,
    });
    if let Err(error) = publication {
        lifecycle
            .controller
            .lock()
            .await
            .finish_error(persistence_failure(&error));
        clear_active(lifecycle).await;
        return Ok(lifecycle.controller.lock().await.view(now_ms()?));
    }

    let maintenance_stage_warning = {
        let mut controller = lifecycle.controller.lock().await;
        controller
            .advance(RefreshStage::Maintaining)
            .err()
            .map(|error| {
                format!(
                    "The snapshot was committed, but the maintenance stage could not be recorded: {error}"
                )
            })
    };
    let prune_warning = persistence
        .0
        .prune(committed_ms, RetentionPolicy::default())
        .err()
        .map(|error| {
            format!(
                "The snapshot was committed, but post-commit retention maintenance failed: {error}"
            )
        });
    {
        let mut controller = lifecycle.controller.lock().await;
        controller.finish_committed_success(committed_ms);
        if let Some(warning) = maintenance_stage_warning {
            controller.record_maintenance_warning(warning);
        }
        if let Some(warning) = prune_warning {
            controller.record_maintenance_warning(warning);
        }
        if let Err(error) = save_lifecycle(&persistence.0, &controller.persisted(), committed_ms) {
            controller.record_maintenance_warning(format!(
                "The snapshot was committed, but lifecycle metadata could not be saved: {}",
                error.message
            ));
        }
    }
    clear_active(lifecycle).await;
    Ok(lifecycle.controller.lock().await.view(now_ms()?))
}

async fn advance_refresh(
    lifecycle: &LifecycleRuntimeState,
    stage: RefreshStage,
    fallback_now_ms: i64,
) -> Result<(), LifecycleView> {
    let mut controller = lifecycle.controller.lock().await;
    if let Err(error) = controller.advance(stage) {
        controller.finish_error(orchestration_failure(&error));
        return Err(controller.view(fallback_now_ms));
    }
    Ok(())
}

async fn clear_active(lifecycle: &LifecycleRuntimeState) {
    *lifecycle.active_cancellation.lock().await = None;
}

fn save_lifecycle(
    store: &p2p_persistence::PersistenceStore,
    value: &PersistedLifecycle,
    now_ms: i64,
) -> Result<(), AppErrorEnvelope> {
    store
        .save_setting(
            SETTINGS_SECTION,
            SETTINGS_KEY,
            &serde_json::to_value(value).map_err(|error| {
                command_error(
                    "LIFECYCLE-SERIALIZE",
                    AppErrorCategory::Internal,
                    error.to_string(),
                    false,
                    None,
                )
            })?,
            now_ms,
        )
        .map_err(|error| persistence_error("LIFECYCLE-SAVE", &error))
}

fn now_ms() -> Result<i64, AppErrorEnvelope> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            command_error(
                "LIFECYCLE-CLOCK",
                AppErrorCategory::Lifecycle,
                format!("System clock is before the Unix epoch: {error}"),
                true,
                None,
            )
        })?;
    i64::try_from(duration.as_millis()).map_err(|_| {
        command_error(
            "LIFECYCLE-CLOCK-RANGE",
            AppErrorCategory::Lifecycle,
            "System clock exceeds the supported range.",
            false,
            None,
        )
    })
}

fn configuration_error(error: LifecycleError) -> AppErrorEnvelope {
    command_error(
        "LIFECYCLE-INVALID-CONTEXT",
        AppErrorCategory::Configuration,
        error.to_string(),
        false,
        None,
    )
}

fn lifecycle_error(error: LifecycleError) -> AppErrorEnvelope {
    command_error(
        "LIFECYCLE-STATE",
        AppErrorCategory::Lifecycle,
        error.to_string(),
        matches!(
            error,
            LifecycleError::RefreshInProgress | LifecycleError::Offline
        ),
        None,
    )
}

fn persistence_error(
    code: &'static str,
    error: &p2p_persistence::PersistenceError,
) -> AppErrorEnvelope {
    command_error(
        code,
        AppErrorCategory::Storage,
        error.to_string(),
        true,
        None,
    )
}

fn provider_failure(error: &ProviderError) -> ActionableFailure {
    ActionableFailure {
        kind: FailureKind::Provider,
        title: "Live provider refresh failed".to_owned(),
        detail: error.to_string(),
        retryable: !matches!(error, ProviderError::Contract(_)),
        action: if matches!(error, ProviderError::CircuitOpen(_)) {
            "Wait for Data Health to allow another request"
        } else {
            "Retry refresh"
        }
        .to_owned(),
    }
}

fn validation_failure(error: &LifecycleError) -> ActionableFailure {
    ActionableFailure {
        kind: FailureKind::Validation,
        title: "Live response validation failed".to_owned(),
        detail: error.to_string(),
        retryable: true,
        action: "Retry with the applied context".to_owned(),
    }
}

fn clock_failure(detail: impl Into<String>) -> ActionableFailure {
    ActionableFailure {
        kind: FailureKind::Validation,
        title: "System clock changed during refresh".to_owned(),
        detail: detail.into(),
        retryable: true,
        action: "Correct the clock, then refresh".to_owned(),
    }
}

fn calculation_failure(error: &LifecycleError) -> ActionableFailure {
    ActionableFailure {
        kind: FailureKind::Calculation,
        title: "Exact calculation failed".to_owned(),
        detail: error.to_string(),
        retryable: true,
        action: "Review filters, then refresh".to_owned(),
    }
}

fn orchestration_failure(error: &LifecycleError) -> ActionableFailure {
    ActionableFailure {
        kind: FailureKind::Validation,
        title: "Refresh lifecycle failed closed".to_owned(),
        detail: error.to_string(),
        retryable: true,
        action: "Retry refresh".to_owned(),
    }
}

fn persistence_failure(error: &p2p_persistence::PersistenceError) -> ActionableFailure {
    ActionableFailure {
        kind: FailureKind::Persistence,
        title: "Validated results could not be committed".to_owned(),
        detail: error.to_string(),
        retryable: true,
        action: "Check local storage, then refresh".to_owned(),
    }
}

fn command_error(
    code: &'static str,
    category: AppErrorCategory,
    message: impl Into<String>,
    retryable: bool,
    request_id: Option<String>,
) -> AppErrorEnvelope {
    AppErrorEnvelope::lifecycle(code, category, message, retryable, request_id)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use p2p_lifecycle::{LifecycleStatus, RefreshSettings};
    use p2p_persistence::{PersistenceStore, RuntimeVersions};

    use super::*;

    static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn temporary_root(label: &str) -> PathBuf {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "p2p-desk-gate5-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn open_store(root: &PathBuf) -> PersistenceStore {
        PersistenceStore::open(
            root,
            RuntimeVersions::current("0.1.0").expect("versions"),
            1_000,
        )
        .expect("store")
    }

    #[test]
    fn first_run_defaults_are_validated_persisted_and_restorable() {
        let root = temporary_root("defaults");
        let store = open_store(&root);
        let runtime = initialize_lifecycle(&store, 1_000).expect("initialize");
        let view =
            tauri::async_runtime::block_on(async { runtime.controller.lock().await.view(1_000) });
        assert_eq!(view.settings, RefreshSettings::default());
        assert!(matches!(view.status, LifecycleStatus::Ready { .. }));
        assert!(
            store
                .load_setting(SETTINGS_SECTION, SETTINGS_KEY)
                .expect("load")
                .is_some()
        );
        drop(store);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn invalid_restored_json_is_visible_and_never_silently_defaulted() {
        let root = temporary_root("invalid");
        let store = open_store(&root);
        store
            .save_setting(
                SETTINGS_SECTION,
                SETTINGS_KEY,
                &serde_json::json!({"version": 1, "settings": {"autoRefresh": true}}),
                1_000,
            )
            .expect("save invalid state");
        let runtime = initialize_lifecycle(&store, 1_001).expect("initialize");
        let view =
            tauri::async_runtime::block_on(async { runtime.controller.lock().await.view(1_001) });
        assert!(matches!(view.status, LifecycleStatus::Error { .. }));
        assert_eq!(view.last_success_ms, None);
        drop(store);
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
