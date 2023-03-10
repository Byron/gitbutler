import type { LayoutLoad } from './$types';
import { building } from '$app/environment';
import { readable, derived } from 'svelte/store';
import type { Session } from '$lib/sessions';
import type { UISession } from '$lib/uisessions';
import type { Status } from '$lib/statuses';
import { asyncDerived } from '@square/svelte-store';
import type { Delta } from '$lib/deltas';
import { startOfDay } from 'date-fns';

export const prerender = false;
export const load: LayoutLoad = async ({ parent, params }) => {
	const { projects } = await parent();

	const filesStatus = building
		? readable<Status[]>([])
		: await (await import('$lib/statuses')).default({ projectId: params.projectId });

	const sessions = building
		? readable<Session[]>([])
		: await (await import('$lib/sessions')).default({ projectId: params.projectId });
	const orderedSessions = derived(sessions, (sessions) => {
		return sessions.slice().sort((a, b) => a.meta.startTimestampMs - b.meta.startTimestampMs);
	});

	let dateSessions = readable<Record<number, UISession[]>>({});
	if (!building) {
		const listDeltas = (await import('$lib/deltas')).list;
		dateSessions = asyncDerived([orderedSessions], async ([sessions]) => {
			const deltas = await Promise.all(
				sessions.map((session) => {
					return listDeltas({
						projectId: params.projectId ?? '',
						sessionId: session.id
					});
				})
			);
			// Sort deltas by timestamp
			deltas.forEach((delta) => {
				Object.keys(delta).forEach((key) => {
					delta[key].sort((a, b) => a.timestampMs - b.timestampMs).reverse();
				});
			});

			const uiSessions = sessions
				.map((session, i) => {
					return { session, deltas: deltas[i] } as UISession;
				})
				.filter((uiSession) => {
					return Object.keys(uiSession.deltas).length > 0;
				});

			const dateSessions: Record<number, UISession[]> = {};
			uiSessions.forEach((uiSession) => {
				const date = startOfDay(new Date(uiSession.session.meta.startTimestampMs));
				if (dateSessions[date.getTime()]) {
					dateSessions[date.getTime()]?.push(uiSession);
				} else {
					dateSessions[date.getTime()] = [uiSession];
				}
			});

			// For each UISession in dateSessions, set the earliestDeltaTimestampMs and latestDeltaTimestampMs
			Object.keys(dateSessions).forEach((date: any) => {
				dateSessions[date].forEach((uiSession: any) => {
					const deltaTimestamps = Object.keys(uiSession.deltas).reduce((acc, key) => {
						return acc.concat(uiSession.deltas[key].map((delta: Delta) => delta.timestampMs));
					}, []);
					uiSession.earliestDeltaTimestampMs = Math.min(...deltaTimestamps);
					uiSession.latestDeltaTimestampMs = Math.max(...deltaTimestamps);
				});
			});

			return dateSessions;
		});
	}

	return {
		project: projects.get(params.projectId),
		projectId: params.projectId,
		sessions: orderedSessions,
		dateSessions: dateSessions,
		filesStatus: filesStatus
	};
};
