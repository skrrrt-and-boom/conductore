//! Task dependency graph analysis using Kahn's algorithm.
//!
//! Detects dependency cycles (which would cause deadlock) and validates
//! plan integrity before execution.

use conductor_types::{PlanValidation, Task, ValidationIssue, ValidationSeverity};
use std::collections::{HashMap, HashSet};

/// Detect dependency cycles in the task graph using Kahn's algorithm.
///
/// Returns an array of cycles found (each cycle is an array of task indices).
/// Empty vec means no cycles (valid DAG).
///
/// Note: Uses array positions internally, not `task.index` values. This allows
/// the function to work correctly when tasks have non-contiguous indices (e.g.,
/// phase 2 tasks with indices 5,6,7,8 in a 4-element slice).
pub fn detect_dependency_cycles(tasks: &[Task]) -> Vec<Vec<usize>> {
    let n = tasks.len();
    let mut in_degree = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];

    // Build mapping from task.index → array position
    let index_to_pos: HashMap<usize, usize> = tasks
        .iter()
        .enumerate()
        .map(|(pos, t)| (t.index, pos))
        .collect();

    // Build adjacency list using array positions
    for (pos, task) in tasks.iter().enumerate() {
        for &dep in &task.dependencies {
            if let Some(&dep_pos) = index_to_pos.get(&dep) {
                adj[dep_pos].push(pos);
                in_degree[pos] += 1;
            }
        }
    }

    // Kahn's algorithm — topological sort
    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut sorted = Vec::new();

    while let Some(node) = queue.first().copied() {
        queue.remove(0);
        sorted.push(node);
        for &neighbor in &adj[node] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push(neighbor);
            }
        }
    }

    // If sorted doesn't contain all nodes, there are cycles
    if sorted.len() == n {
        return vec![];
    }

    // Find the actual cycle nodes (those not in sorted)
    let in_sorted: HashSet<usize> = sorted.into_iter().collect();
    let cycle_nodes: Vec<usize> = (0..n).filter(|i| !in_sorted.contains(i)).collect();

    // Group connected cycle nodes into individual cycles via DFS
    let mut visited = HashSet::new();
    let mut cycles = Vec::new();

    for &start in &cycle_nodes {
        if visited.contains(&start) {
            continue;
        }
        let mut cycle = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            if visited.contains(&node) {
                continue;
            }
            visited.insert(node);
            cycle.push(node);
            for &neighbor in &adj[node] {
                if !in_sorted.contains(&neighbor) && !visited.contains(&neighbor) {
                    stack.push(neighbor);
                }
            }
        }
        if !cycle.is_empty() {
            cycles.push(cycle);
        }
    }

    cycles
}

/// Validate a plan before execution.
///
/// Checks for:
/// 1. Invalid dependency references (non-existent task.index, self-dependency)
/// 2. Dependency cycles (would cause deadlock)
/// 3. Overlapping file_scope without dependency links (merge conflict risk)
///
/// Note: Uses task.index values for dependency validation, not array positions.
/// This correctly handles non-contiguous indices (e.g., phase 2 tasks with
/// indices 5,6,7,8).
pub fn validate_plan(tasks: &[Task]) -> PlanValidation {
    let mut issues = Vec::new();

    // Build set of valid task indices for dependency validation
    let valid_indices: HashSet<usize> = tasks.iter().map(|t| t.index).collect();

    // 1. Check for invalid dependency references
    for task in tasks {
        for &dep in &task.dependencies {
            if !valid_indices.contains(&dep) {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!(
                        "Task {} (\"{}\") references non-existent dependency index {}",
                        task.index, task.title, dep
                    ),
                    task_indices: Some(vec![task.index]),
                });
            }
            if dep == task.index {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!(
                        "Task {} (\"{}\") depends on itself",
                        task.index, task.title
                    ),
                    task_indices: Some(vec![task.index]),
                });
            }
        }
    }

    // 2. Dependency cycle detection
    let cycles = detect_dependency_cycles(tasks);
    for cycle in &cycles {
        // cycles contain array positions, convert to task.index for reporting
        let names: Vec<String> = cycle
            .iter()
            .map(|&pos| {
                let task = &tasks[pos];
                format!("{} (\"{}\")", task.index, task.title)
            })
            .collect();
        let task_indices: Vec<usize> = cycle.iter().map(|&pos| tasks[pos].index).collect();
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            message: format!("Dependency cycle detected: {}", names.join(" → ")),
            task_indices: Some(task_indices),
        });
    }

    // 3. Overlapping file_scope without dependency links (warning)
    for i in 0..tasks.len() {
        for j in (i + 1)..tasks.len() {
            let overlap: Vec<&String> = tasks[i]
                .file_scope
                .iter()
                .filter(|f| tasks[j].file_scope.contains(f))
                .collect();
            if !overlap.is_empty() {
                // Check if either task depends on the other (using task.index)
                let has_dep_link = tasks[i].dependencies.contains(&tasks[j].index)
                    || tasks[j].dependencies.contains(&tasks[i].index);
                if !has_dep_link {
                    let files: Vec<&str> = overlap.iter().map(|s| s.as_str()).collect();
                    issues.push(ValidationIssue {
                        severity: ValidationSeverity::Warning,
                        message: format!(
                            "Tasks {} (\"{}\") and {} (\"{}\") share files [{}] but have no dependency — merge conflict risk",
                            tasks[i].index, tasks[i].title, tasks[j].index, tasks[j].title, files.join(", ")
                        ),
                        task_indices: Some(vec![tasks[i].index, tasks[j].index]),
                    });
                }
            }
        }
    }

    let has_errors = issues.iter().any(|i| i.severity == ValidationSeverity::Error);
    let cycles_opt = if cycles.is_empty() {
        None
    } else {
        Some(cycles)
    };

    PlanValidation {
        valid: !has_errors,
        issues,
        cycles: cycles_opt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conductor_types::TaskStatus;

    fn make_task(index: usize, dependencies: Vec<usize>, file_scope: Vec<String>) -> Task {
        Task {
            id: format!("task-{index}"),
            index,
            title: format!("Task {index}"),
            description: String::new(),
            why: String::new(),
            file_scope,
            dependencies,
            acceptance_criteria: vec![],
            estimated_turns: 10,
            model: None,
            status: TaskStatus::Queued,
            assigned_musician: None,
            result: None,
        }
    }

    #[test]
    fn valid_dag_returns_empty() {
        let tasks = vec![
            make_task(0, vec![], vec![]),
            make_task(1, vec![0], vec![]),
            make_task(2, vec![1], vec![]),
        ];
        assert!(detect_dependency_cycles(&tasks).is_empty());
    }

    #[test]
    fn independent_tasks_returns_empty() {
        let tasks = vec![
            make_task(0, vec![], vec![]),
            make_task(1, vec![], vec![]),
            make_task(2, vec![], vec![]),
        ];
        assert!(detect_dependency_cycles(&tasks).is_empty());
    }

    #[test]
    fn detects_two_node_cycle() {
        let tasks = vec![
            make_task(0, vec![1], vec![]),
            make_task(1, vec![0], vec![]),
        ];
        let cycles = detect_dependency_cycles(&tasks);
        assert!(!cycles.is_empty());
        assert!(cycles[0].contains(&0));
        assert!(cycles[0].contains(&1));
    }

    #[test]
    fn detects_three_node_cycle() {
        let tasks = vec![
            make_task(0, vec![2], vec![]),
            make_task(1, vec![0], vec![]),
            make_task(2, vec![1], vec![]),
        ];
        let cycles = detect_dependency_cycles(&tasks);
        assert!(!cycles.is_empty());
    }

    #[test]
    fn handles_self_dependency() {
        let tasks = vec![make_task(0, vec![0], vec![])];
        let cycles = detect_dependency_cycles(&tasks);
        assert!(!cycles.is_empty());
    }

    #[test]
    fn ignores_out_of_bounds_dependencies() {
        let tasks = vec![
            make_task(0, vec![99], vec![]),
            make_task(1, vec![0], vec![]),
        ];
        assert!(detect_dependency_cycles(&tasks).is_empty());
    }

    #[test]
    fn validate_clean_plan() {
        let tasks = vec![
            make_task(0, vec![], vec!["a.ts".into()]),
            make_task(1, vec![0], vec!["b.ts".into()]),
        ];
        let result = validate_plan(&tasks);
        assert!(result.valid);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn validate_detects_invalid_dependency() {
        let tasks = vec![make_task(0, vec![5], vec![])];
        let result = validate_plan(&tasks);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.severity == ValidationSeverity::Error));
    }

    #[test]
    fn validate_detects_self_dependency() {
        let tasks = vec![make_task(0, vec![0], vec![])];
        let result = validate_plan(&tasks);
        assert!(!result.valid);
    }

    #[test]
    fn validate_warns_on_overlapping_file_scope() {
        let tasks = vec![
            make_task(0, vec![], vec!["shared.ts".into()]),
            make_task(1, vec![], vec!["shared.ts".into()]),
        ];
        let result = validate_plan(&tasks);
        assert!(result.valid); // warnings don't make it invalid
        assert!(result.issues.iter().any(|i| i.severity == ValidationSeverity::Warning));
    }

    #[test]
    fn validate_no_warning_when_overlap_has_dep() {
        let tasks = vec![
            make_task(0, vec![], vec!["shared.ts".into()]),
            make_task(1, vec![0], vec!["shared.ts".into()]),
        ];
        let result = validate_plan(&tasks);
        assert!(result.valid);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn handles_non_contiguous_indices() {
        // Simulates phase 2 tasks with global indices 5,6,7,8 (not 0,1,2,3)
        // This was causing a panic before the fix
        let tasks = vec![
            make_task(5, vec![], vec![]),
            make_task(6, vec![5], vec![]),
            make_task(7, vec![6], vec![]),
            make_task(8, vec![7], vec![]),
        ];
        assert!(detect_dependency_cycles(&tasks).is_empty());
        let result = validate_plan(&tasks);
        assert!(result.valid);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn non_contiguous_detects_cycle() {
        // Cycle: 10 → 11 → 12 → 10
        let tasks = vec![
            make_task(10, vec![12], vec![]),
            make_task(11, vec![10], vec![]),
            make_task(12, vec![11], vec![]),
        ];
        let cycles = detect_dependency_cycles(&tasks);
        assert!(!cycles.is_empty());
        let result = validate_plan(&tasks);
        assert!(!result.valid);
    }

    #[test]
    fn non_contiguous_overlap_warning() {
        // Two tasks with non-contiguous indices sharing a file, no dependency
        let tasks = vec![
            make_task(5, vec![], vec!["shared.rs".into()]),
            make_task(8, vec![], vec!["shared.rs".into()]),
        ];
        let result = validate_plan(&tasks);
        assert!(result.valid); // warnings don't invalidate
        assert!(result.issues.iter().any(|i| i.severity == ValidationSeverity::Warning));
    }

    #[test]
    fn non_contiguous_overlap_with_dep_no_warning() {
        // Two tasks with non-contiguous indices sharing a file, WITH dependency
        let tasks = vec![
            make_task(5, vec![], vec!["shared.rs".into()]),
            make_task(8, vec![5], vec!["shared.rs".into()]),
        ];
        let result = validate_plan(&tasks);
        assert!(result.valid);
        assert!(result.issues.is_empty());
    }
}
