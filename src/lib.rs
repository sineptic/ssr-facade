use std::time::SystemTime;
use std::{collections::BTreeSet, time::Duration};

use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use ssr_core::{
    task::{SharedStateExt, Task},
    tasks_facade::TasksFacade,
};

#[derive(Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Task<'de>"))]
#[serde(transparent)]
struct TaskWraper<T>(T);
impl<'a, T: Task<'a>> PartialEq for TaskWraper<T> {
    fn eq(&self, other: &Self) -> bool {
        (self.0.next_repetition(0.5)) == (other.0.next_repetition(0.5))
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
        (self.0.next_repetition(0.5)).cmp(&other.0.next_repetition(0.5))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(bound(deserialize = "'a: 'de, 'de: 'a"))]
pub struct Facade<'a, T>
where
    T: Task<'a>,
{
    name: String,
    tasks_pool: BTreeSet<TaskWraper<T>>,
    tasks_to_recall: Vec<TaskWraper<T>>,
    target_recall: f64,
    state: T::SharedState,
}

impl<'a, T: Task<'a>> Facade<'a, T> {
    pub fn find_tasks_to_recall(&mut self) {
        while let Some(task) = self.tasks_pool.pop_first() {
            if task.0.next_repetition(self.target_recall)
                <= SystemTime::now() + Duration::from_secs(10)
            {
                self.tasks_to_recall.push(task);
            } else {
                self.tasks_pool.insert(task);
                break;
            }
        }
    }

    fn take_random_task(&mut self) -> Option<TaskWraper<T>> {
        if self.tasks_to_recall.is_empty() {
            return None;
        }
        let index = thread_rng().gen_range(0..self.tasks_to_recall.len());
        Some(self.tasks_to_recall.swap_remove(index))
    }
}
impl<'a, T: Task<'a>> TasksFacade<'a, T> for Facade<'a, T> {
    fn new(name: String) -> Self {
        Self {
            name,
            tasks_pool: Default::default(),
            tasks_to_recall: Default::default(),
            target_recall: 0.8,
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
            s_text_input_f::Blocks,
        ) -> std::io::Result<s_text_input_f::Response>,
    ) -> Result<(), ssr_core::tasks_facade::Error> {
        self.find_tasks_to_recall();
        if let Some(TaskWraper(mut task)) = self.take_random_task() {
            task.complete(&mut self.state, interaction)?;
            self.tasks_pool.insert(TaskWraper(task));
            Ok(())
        } else {
            match self
                .tasks_pool
                .first()
                .map(|TaskWraper(x)| x.next_repetition(self.target_recall))
            {
                Some(next_repetition) => Err(ssr_core::tasks_facade::Error::NoTaskToComplete {
                    time_until_next_repetition: next_repetition.elapsed().unwrap(),
                }),
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

impl<'a, T: Task<'a>> Facade<'a, T>
where
    T::SharedState: SharedStateExt<'a>,
{
    pub fn optimize(&mut self) {
        self.state.optimize()
    }
}
