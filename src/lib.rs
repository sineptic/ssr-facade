#![warn(clippy::pedantic)]
#![feature(extract_if, btree_extract_if)]

use rand::{thread_rng, Rng};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ssr_core::{
    task::{SharedStateExt, Task},
    tasks_facade::{TaskId, TasksFacade},
};
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime},
};

fn serialize_id<S>(id: &TaskId, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    id.serialize(serializer)
}
fn deserialize_id<'de, D>(deserializer: D) -> Result<TaskId, D::Error>
where
    D: Deserializer<'de>,
{
    const GEN_RANDOM: bool = false;

    let id = TaskId::deserialize(deserializer)?;
    let id = if GEN_RANDOM { rand::random() } else { id };
    Ok(id)
}

#[derive(Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Task<'de>"))]
struct TaskWrapper<T> {
    task: T,
    #[serde(serialize_with = "serialize_id", deserialize_with = "deserialize_id")]
    id: TaskId,
}

impl<'a, T: Task<'a>> PartialEq for TaskWrapper<T> {
    fn eq(&self, other: &Self) -> bool {
        let shared_state = Default::default();
        (self.task.next_repetition(&shared_state, 0.5))
            == (other.task.next_repetition(&shared_state, 0.5))
    }
}
impl<'a, T: Task<'a>> Eq for TaskWrapper<T> {}
impl<'a, T: Task<'a>> PartialOrd for TaskWrapper<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<'a, T: Task<'a>> Ord for TaskWrapper<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let shared_state = Default::default();
        (self.task.next_repetition(&shared_state, 0.5))
            .cmp(&other.task.next_repetition(&shared_state, 0.5))
    }
}
impl<'a, T: Task<'a>> TaskWrapper<T> {
    fn new(value: T) -> Self {
        Self {
            task: value,
            id: rand::random(),
        }
    }
}
// FIXME: move Ord to Task trait

#[derive(Serialize, Deserialize)]
#[serde(bound(deserialize = "'a: 'de, 'de: 'a"))]
pub struct Facade<'a, T>
where
    T: Task<'a>,
{
    name: String,
    tasks_pool: BTreeSet<TaskWrapper<T>>,
    tasks_to_recall: Vec<TaskWrapper<T>>,
    desired_retention: f64,
    state: T::SharedState,
}

impl<'a, T: Task<'a>> Facade<'a, T> {
    pub fn find_tasks_to_recall(&mut self) {
        while let Some(task) = self.tasks_pool.pop_first() {
            let now = SystemTime::now() + Duration::from_secs(10);
            if task
                .task
                .next_repetition(&self.state, self.desired_retention)
                <= now
            {
                self.tasks_to_recall.push(task);
            } else {
                self.tasks_pool.insert(task);
                break;
            }
        }
    }
    pub fn reload_all_tasks_timings(&mut self) {
        let now = SystemTime::now();
        let not_to_recall = self
            .tasks_to_recall
            .extract_if(|x| x.task.next_repetition(&self.state, self.desired_retention) > now);
        self.tasks_pool.extend(not_to_recall);
        let to_recall = self
            .tasks_pool
            .extract_if(|x| x.task.next_repetition(&self.state, self.desired_retention) < now);
        self.tasks_to_recall.extend(to_recall);
    }

    fn take_random_task(&mut self) -> Option<TaskWrapper<T>> {
        if self.tasks_to_recall.is_empty() {
            return None;
        }
        let index = thread_rng().gen_range(0..self.tasks_to_recall.len());
        Some(self.tasks_to_recall.swap_remove(index))
    }

    pub fn until_next_repetition(&self) -> Option<Duration> {
        if self.tasks_to_complete() > 0 {
            None
        } else {
            self.tasks_pool
                .first()?
                .task
                .next_repetition(&self.state, self.desired_retention)
                .duration_since(SystemTime::now())
                .ok()
        }
    }
}
impl<'a, F: Task<'a>> Facade<'a, F> {
    /// # Warning
    /// You will loose all progress.
    pub fn migrate<T: Task<'a>>(&self) -> Facade<'a, T> {
        let task_templates = self
            .tasks_pool
            .iter()
            .chain(self.tasks_to_recall.iter())
            .map(|t| t.task.get_blocks());
        let mut new_facade = Facade::new(self.name.clone(), self.desired_retention);
        for i in task_templates {
            new_facade.create_task(i);
        }
        new_facade
    }
}
impl<'a, T: Task<'a>> TasksFacade<'a, T> for Facade<'a, T> {
    fn new(name: String, desired_retention: f64) -> Self {
        Self {
            name,
            tasks_pool: BTreeSet::default(),
            tasks_to_recall: Vec::default(),
            desired_retention,
            state: T::SharedState::default(),
        }
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn tasks_total(&self) -> usize {
        self.tasks_pool.len() + self.tasks_to_recall.len()
    }
    fn tasks_to_complete(&self) -> usize {
        self.tasks_to_recall.len()
    }

    fn complete_task(
        &mut self,
        interaction: &mut impl FnMut(
            TaskId,
            s_text_input_f::Blocks,
        ) -> std::io::Result<s_text_input_f::Response>,
    ) -> Result<(), ssr_core::tasks_facade::Error> {
        self.find_tasks_to_recall();
        if let Some(TaskWrapper { mut task, id }) = self.take_random_task() {
            task.complete(&mut self.state, self.desired_retention, &mut |blocks| {
                interaction(id, blocks)
            })?;
            self.tasks_pool.insert(TaskWrapper { task, id });
            Ok(())
        } else {
            match self.tasks_pool.first().map(|TaskWrapper { task, id: _ }| {
                task.next_repetition(&self.state, self.desired_retention)
            }) {
                Some(next_repetition) => Err(ssr_core::tasks_facade::Error::NoTaskToComplete {
                    time_until_next_repetition: next_repetition
                        .duration_since(SystemTime::now())
                        .unwrap_or_default(),
                }),
                None => Err(ssr_core::tasks_facade::Error::NoTask),
            }
        }
    }

    fn insert(&mut self, task: T) {
        self.tasks_pool.insert(TaskWrapper::new(task));
    }

    fn iter<'t>(&'t self) -> impl Iterator<Item = (&'t T, TaskId)>
    where
        T: 't,
    {
        self.tasks_pool
            .iter()
            .chain(self.tasks_to_recall.iter())
            .map(|TaskWrapper { task, id }| (task, *id))
    }

    fn remove(&mut self, id: TaskId) -> bool {
        let mut removed = false;
        self.tasks_to_recall.retain(|task_wrapper| {
            if task_wrapper.id == id {
                removed = true;
                false
            } else {
                true
            }
        });
        if !removed {
            self.tasks_pool.retain(|task_wrapper| {
                if task_wrapper.id == id {
                    removed = true;
                    false
                } else {
                    true
                }
            });
        }
        removed
    }

    fn get_desired_retention(&self) -> f64 {
        self.desired_retention
    }

    fn set_desired_retention(&mut self, desired_retention: f64) {
        self.desired_retention = desired_retention;

        self.reload_all_tasks_timings();
    }

    fn create_task(&mut self, input: s_text_input_f::BlocksWithAnswer) {
        self.insert(T::new(input));
    }
}

impl<'a, T: Task<'a>> Facade<'a, T>
where
    T::SharedState: SharedStateExt<'a>,
{
    pub fn optimize(&mut self) {
        self.state.optimize();
    }
}
