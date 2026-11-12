//! Deterministic virtual process and thread identities.

use std::collections::BTreeMap;

use nix::unistd::Pid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualIds {
    pub vpid: u32,
    pub vppid: u32,
    pub vtid: u32,
}

#[derive(Debug)]
pub struct VirtualIdManager {
    next_pid: u32,
    next_tid: u32,
    ids: BTreeMap<Pid, VirtualIds>,
}

impl VirtualIdManager {
    pub fn new(root_pid: Pid) -> Self {
        Self {
            next_pid: 1001,
            next_tid: 1001,
            ids: BTreeMap::from([(
                root_pid,
                VirtualIds {
                    vpid: 1000,
                    vppid: 999,
                    vtid: 1000,
                },
            )]),
        }
    }

    /// Register a child using the independent process or thread ID sequence.
    /// Panics if that sequence is exhausted, rather than wrapping and reusing IDs.
    pub fn register_child(
        &mut self,
        parent_pid: Pid,
        child_pid: Pid,
        is_thread: bool,
    ) -> VirtualIds {
        let parent_vpid = self.get_vpid(parent_pid);
        let ids = if is_thread {
            let vtid = self.next_tid;
            self.next_tid = self
                .next_tid
                .checked_add(1)
                .expect("virtual thread ID space exhausted");
            VirtualIds {
                vpid: parent_vpid,
                vppid: self.get_vppid(parent_pid),
                vtid,
            }
        } else {
            let vpid = self.next_pid;
            self.next_pid = self
                .next_pid
                .checked_add(1)
                .expect("virtual process ID space exhausted");
            VirtualIds {
                vpid,
                vppid: parent_vpid,
                vtid: vpid,
            }
        };
        self.ids.insert(child_pid, ids);
        ids
    }

    pub fn get_vpid(&self, pid: Pid) -> u32 {
        self.ids.get(&pid).map_or(1000, |ids| ids.vpid)
    }

    pub fn get_vppid(&self, pid: Pid) -> u32 {
        self.ids.get(&pid).map_or(999, |ids| ids.vppid)
    }

    pub fn get_vtid(&self, pid: Pid) -> u32 {
        self.ids
            .get(&pid)
            .map_or_else(|| self.get_vpid(pid), |ids| ids.vtid)
    }

    pub fn real_pid_for_vpid(&self, vpid: u32) -> Option<Pid> {
        self.ids.iter().find_map(
            |(pid, ids)| {
                if ids.vpid == vpid { Some(*pid) } else { None }
            },
        )
    }

    pub fn remove(&mut self, pid: Pid) {
        self.ids.remove(&pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(value: i32) -> Pid {
        Pid::from_raw(value)
    }

    fn ids(manager: &VirtualIdManager, task: Pid) -> VirtualIds {
        VirtualIds {
            vpid: manager.get_vpid(task),
            vppid: manager.get_vppid(task),
            vtid: manager.get_vtid(task),
        }
    }

    #[test]
    fn root_and_unregistered_tasks_have_expected_ids() {
        let manager = VirtualIdManager::new(pid(42));
        assert_eq!(
            ids(&manager, pid(42)),
            VirtualIds {
                vpid: 1000,
                vppid: 999,
                vtid: 1000
            }
        );
        assert_eq!(ids(&manager, pid(99)), ids(&manager, pid(42)));
        assert_eq!(manager.ids.len(), 1);
    }

    #[test]
    fn threads_inherit_process_identity_while_processes_get_new_ids() {
        let mut manager = VirtualIdManager::new(pid(10));
        assert_eq!(
            manager.register_child(pid(10), pid(11), true),
            VirtualIds {
                vpid: 1000,
                vppid: 999,
                vtid: 1001
            }
        );
        assert_eq!(
            manager.register_child(pid(11), pid(12), true),
            VirtualIds {
                vpid: 1000,
                vppid: 999,
                vtid: 1002
            }
        );
        assert_eq!(
            manager.register_child(pid(11), pid(20), false),
            VirtualIds {
                vpid: 1001,
                vppid: 1000,
                vtid: 1001
            }
        );
        assert_eq!(
            manager.register_child(pid(20), pid(21), true),
            VirtualIds {
                vpid: 1001,
                vppid: 1000,
                vtid: 1003
            }
        );
        let grandchild = manager.register_child(pid(21), pid(30), false);
        assert_eq!(
            grandchild,
            VirtualIds {
                vpid: 1002,
                vppid: 1001,
                vtid: 1002
            }
        );
        assert_eq!(ids(&manager, pid(30)), grandchild);
        assert_eq!(
            ids(&manager, pid(10)),
            VirtualIds {
                vpid: 1000,
                vppid: 999,
                vtid: 1000
            }
        );
    }

    #[test]
    fn unknown_parents_use_fallbacks_without_being_registered() {
        let mut manager = VirtualIdManager::new(pid(10));
        assert_eq!(
            manager.register_child(pid(99), pid(20), false),
            VirtualIds {
                vpid: 1001,
                vppid: 1000,
                vtid: 1001
            }
        );
        assert_eq!(
            manager.register_child(pid(99), pid(21), true),
            VirtualIds {
                vpid: 1000,
                vppid: 999,
                vtid: 1001
            }
        );
        assert!(!manager.ids.contains_key(&pid(99)));
    }

    #[test]
    fn removal_does_not_reuse_ids_or_remove_descendants() {
        let mut manager = VirtualIdManager::new(pid(10));
        manager.register_child(pid(10), pid(20), false);
        let thread = manager.register_child(pid(20), pid(21), true);
        manager.remove(pid(20));
        manager.remove(pid(20)); // removing an unknown task is harmless
        assert_eq!(
            ids(&manager, pid(20)),
            VirtualIds {
                vpid: 1000,
                vppid: 999,
                vtid: 1000
            }
        );
        assert_eq!(ids(&manager, pid(21)), thread);
        assert_eq!(manager.register_child(pid(10), pid(20), false).vpid, 1002);
        manager.remove(pid(21));
        assert_eq!(manager.register_child(pid(20), pid(21), true).vtid, 1002);
    }

    #[test]
    fn allocations_do_not_depend_on_host_pid_values() {
        let mut a = VirtualIdManager::new(pid(10));
        let mut b = VirtualIdManager::new(pid(900));
        assert_eq!(
            a.register_child(pid(10), pid(11), false),
            b.register_child(pid(900), pid(3), false)
        );
        assert_eq!(
            a.register_child(pid(11), pid(12), true),
            b.register_child(pid(3), pid(999), true)
        );
        assert_eq!(
            a.register_child(pid(12), pid(13), false),
            b.register_child(pid(999), pid(1), false)
        );
    }
}
