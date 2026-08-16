use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct GradingComponent {
    pub name: String,
    pub weight_percent: f64,
    pub category: String,
}

pub fn normalize_course(course: &str) -> String {
    let re = Regex::new(r"(?i)(CMSC|COMM|MATH|PHYS|STAT)\s*(\d+)").unwrap();
    if let Some(caps) = re.captures(course) {
        format!("{} {}", caps.get(1).unwrap().as_str().to_uppercase(), caps.get(2).unwrap().as_str())
    } else {
        course.trim().to_uppercase()
    }
}

pub fn match_grade_to_component<'a>(
    course_src: &str,
    assignment_name: &str,
    components: &'a [GradingComponent],
) -> Option<&'a GradingComponent> {
    let assignment_lower = assignment_name.to_lowercase();
    let course_src_lower = course_src.to_lowercase();
    let is_lab_source = course_src_lower.contains("lab") || course_src_lower.contains("st-4l");

    // First try: If it's a lab source, prioritize components with "lab" in name or category
    if is_lab_source {
        // Look for component containing "lab" AND matching assignment keywords (e.g. "exercise", "project")
        for comp in components {
            let comp_name_lower = comp.name.to_lowercase();
            let comp_cat_lower = comp.category.to_lowercase();
            if comp_cat_lower == "lab" || comp_name_lower.contains("lab") {
                if comp_name_lower.contains("exercise") && assignment_lower.contains("exercise") {
                    return Some(comp);
                }
                if comp_name_lower.contains("project") && assignment_lower.contains("project") {
                    return Some(comp);
                }
            }
        }
    }

    // Second try: Keyword-based matches
    for comp in components {
        let comp_name_lower = comp.name.to_lowercase();
        
        // Exact or substring match of the assignment name in component
        if comp_name_lower.contains("quiz") && assignment_lower.contains("quiz") {
            return Some(comp);
        }
        if comp_name_lower.contains("assignment") && assignment_lower.contains("assignment") {
            return Some(comp);
        }
        if comp_name_lower.contains("exam") && assignment_lower.contains("exam") {
            return Some(comp);
        }
        if comp_name_lower.contains("exercise") && assignment_lower.contains("exercise") {
            return Some(comp);
        }
        if comp_name_lower.contains("project") && assignment_lower.contains("project") {
            return Some(comp);
        }
    }

    // Third try: Fallback search for any overlap
    for comp in components {
        let comp_name_lower = comp.name.to_lowercase();
        let words: Vec<&str> = comp_name_lower.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
        for word in words {
            if word.len() > 3 && assignment_lower.contains(word) {
                return Some(comp);
            }
        }
    }

    // Default to the first component if none matched, or None
    components.first()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_course() {
        assert_eq!(normalize_course("CMSC 124 ST-4L"), "CMSC 124");
        assert_eq!(normalize_course("CMSC 124 1st Sem 2026-2027"), "CMSC 124");
        assert_eq!(normalize_course("CMSC 132 Lab"), "CMSC 132");
        assert_eq!(normalize_course("comm 10"), "COMM 10");
    }

    #[test]
    fn test_match_grade_to_component() {
        let components = vec![
            GradingComponent {
                name: "Quizzes".to_string(),
                weight_percent: 10.0,
                category: "Lecture".to_string(),
            },
            GradingComponent {
                name: "Lab Exercises".to_string(),
                weight_percent: 35.0,
                category: "Lab".to_string(),
            },
            GradingComponent {
                name: "Project".to_string(),
                weight_percent: 15.0,
                category: "Lab".to_string(),
            },
        ];

        // Quiz mapping
        let matched = match_grade_to_component(
            "CMSC 124 1st Sem 2026-2027",
            "Quiz 01 - Programming Languages and Categories",
            &components,
        );
        assert_eq!(matched.unwrap().name, "Quizzes");

        // Lab Exercise mapping
        let matched = match_grade_to_component(
            "CMSC 124 ST-4L",
            "EXERCISE 1: COBOL",
            &components,
        );
        assert_eq!(matched.unwrap().name, "Lab Exercises");
    }
}
