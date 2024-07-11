use std::collections::BTreeSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ssr_core::task::{Feedback, Task, UserInteraction};
use ssr_core::tasks_facade::TasksFacade;

#[derive(Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Task<'de>"))]
#[serde(transparent)]
struct TaskWraper<T>(T);
impl<'a, T: Task<'a>> PartialEq for TaskWraper<T> {
    fn eq(&self, other: &Self) -> bool {
        (self.0.until_next_repetition()) == (other.0.until_next_repetition())
    }
}
impl<'a, T: Task<'a>> Eq for TaskWraper<T> {}
impl<'a, T: Task<'a>> PartialOrd for TaskWraper<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<'a, T: Task<'a>> Ord for TaskWraper<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.0.until_next_repetition()).cmp(&other.0.until_next_repetition())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Task<'de>"))]
pub struct Facade<T> {
    name: String,
    tasks_pool: BTreeSet<TaskWraper<T>>,
    tasks_to_recall: BTreeSet<TaskWraper<T>>,
}

impl<'a, T: Task<'a>> Facade<T> {
    fn find_tasks_to_recall(&mut self) {
        while let Some(task) = self.tasks_pool.pop_first() {
            if task.0.until_next_repetition() != Duration::default() {
                self.tasks_to_recall.insert(task);
            } else {
                self.tasks_pool.insert(task);
                break;
            }
        }
    }
}
impl<'a, T: Task<'a>> TasksFacade<'a, T> for Facade<T> {
    fn new(name: String) -> Self {
        Self {
            name,
            tasks_pool: Default::default(),
            tasks_to_recall: Default::default(),
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
        interaction: impl UserInteraction,
    ) -> Result<Feedback, ssr_core::tasks_facade::Error> {
        self.find_tasks_to_recall();
        if let Some(TaskWraper(task)) = self.tasks_to_recall.pop_first() {
            let (task, feedback) = task.complete(interaction);
            self.tasks_pool.insert(TaskWraper(task));
            Ok(feedback)
        } else {
            match self
                .tasks_pool
                .first()
                .map(|TaskWraper(x)| x.until_next_repetition())
            {
                Some(until_next_repetition) => {
                    Err(ssr_core::tasks_facade::Error::NoTaskToComplete {
                        time_until_next_repetition: until_next_repetition,
                    })
                }
                None => Err(ssr_core::tasks_facade::Error::NoTask),
            }
        }
    }

    fn insert(&mut self, task: T) {
        self.tasks_pool.insert(TaskWraper(task));
    }

    fn iter<'t>(&'t self) -> impl Iterator<Item = &'t T>
    where
        T: 't,
    {
        self.tasks_pool
            .iter()
            .chain(self.tasks_to_recall.iter())
            .map(|TaskWraper(x)| x)
    }

    fn remove(&mut self, _task: &T) -> bool {
        todo!()
    }
}
