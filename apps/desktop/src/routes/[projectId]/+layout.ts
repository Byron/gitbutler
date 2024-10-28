import { ForgeService } from '$lib/backend/forge';
import { getUserErrorCode, invoke } from '$lib/backend/ipc';
import { ProjectService, type Project } from '$lib/backend/projects';
import { BaseBranchService } from '$lib/baseBranch/baseBranchService';
import { CloudBranchCreationService } from '$lib/branch/cloudBranchCreationService';
import { BranchListingService } from '$lib/branches/branchListing';
import { BranchDragActionsFactory } from '$lib/branches/dragActions.js';
import { CommitDragActionsFactory } from '$lib/commits/dragActions.js';
import { CommitService } from '$lib/commits/service';
import { ReorderDropzoneManagerFactory } from '$lib/dragging/reorderDropzoneManager';
import { FetchSignal } from '$lib/fetchSignal/fetchSignal.js';
import { HistoryService } from '$lib/history/history';
import { SyncedSnapshotService } from '$lib/history/syncedSnapshotService';
import { ProjectMetrics } from '$lib/metrics/projectMetrics';
import { ModeService } from '$lib/modes/service';
import { RemoteBranchService } from '$lib/stores/remoteBranches';
import { UncommitedFilesWatcher } from '$lib/uncommitedFiles/watcher';
import { BranchController } from '$lib/vbranches/branchController';
import { UpstreamIntegrationService } from '$lib/vbranches/upstreamIntegrationService';
import { VirtualBranchService } from '$lib/vbranches/virtualBranch';
import { BranchesApiService, CloudBranchesService } from '@gitbutler/shared/cloud/stacks/service';
import { error } from '@sveltejs/kit';
import { derived } from 'svelte/store';
import type { LayoutLoad } from './$types';

export const prerender = false;

// eslint-disable-next-line
export const load: LayoutLoad = async ({ params, parent }) => {
	const { authService, projectsService, cloud, commandService, userService } = await parent();

	const projectId = params.projectId;
	projectsService.setLastOpenedProject(projectId);

	// Getting the project should be one of few, if not the only await expression in
	// this function. It delays drawing the page, but currently the benefit from having this
	// synchronously available are much greater than the cost.
	// However, what's awaited here is required for proper error handling,
	// and by now this is fast enough to not be an impediment.
	let project: Project | undefined = undefined;
	try {
		project = await projectsService.getProject(projectId);
		await invoke('set_project_active', { id: projectId });
	} catch (err: any) {
		const errorCode = getUserErrorCode(err);
		throw error(400, {
			errorCode,
			message: err.message
		});
	}

	const projectService = new ProjectService(projectsService, projectId);

	const projectMetrics = new ProjectMetrics(projectId);

	const modeService = new ModeService(projectId);
	const fetchSignal = new FetchSignal(projectId);

	const historyService = new HistoryService(projectId);
	const baseBranchService = new BaseBranchService(projectId);
	const commitService = new CommitService(projectId);
	const forgeService = new ForgeService(projectId);

	const branchListingService = new BranchListingService(projectId);
	const remoteBranchService = new RemoteBranchService(
		projectId,
		branchListingService,
		projectMetrics
	);

	const vbranchService = new VirtualBranchService(
		projectId,
		projectMetrics,
		remoteBranchService,
		branchListingService
	);

	const branchController = new BranchController(
		projectId,
		vbranchService,
		remoteBranchService,
		baseBranchService
	);

	const branchDragActionsFactory = new BranchDragActionsFactory(branchController);
	const commitDragActionsFactory = new CommitDragActionsFactory(branchController, project);
	const reorderDropzoneManagerFactory = new ReorderDropzoneManagerFactory(branchController);

	const uncommitedFileWatcher = new UncommitedFilesWatcher(project);
	const upstreamIntegrationService = new UpstreamIntegrationService(project, vbranchService);
	const repositoryId = derived(projectsService.getProjectStore(projectId), (project) => {
		return project?.api?.repository_id;
	});
	const syncedSnapshotService = new SyncedSnapshotService(
		commandService,
		userService.user,
		projectsService.getProjectStore(projectId)
	);
	const branchesApiService = new BranchesApiService(cloud);
	const cloudBranchesService = new CloudBranchesService(repositoryId, branchesApiService);
	const cloudBranchCreationService = new CloudBranchCreationService(
		syncedSnapshotService,
		cloudBranchesService
	);

	return {
		authService,
		baseBranchService,
		commitService,
		forgeService,
		branchController,
		historyService,
		projectId,
		project,
		projectService,
		remoteBranchService,
		vbranchService,
		projectMetrics,
		modeService,
		fetchSignal,
		upstreamIntegrationService,

		// These observables are provided for convenience
		branchDragActionsFactory,
		commitDragActionsFactory,
		reorderDropzoneManagerFactory,
		branchListingService,
		uncommitedFileWatcher,

		// Cloud-related services
		syncedSnapshotService,
		cloudBranchesService,
		cloudBranchCreationService
	};
};
