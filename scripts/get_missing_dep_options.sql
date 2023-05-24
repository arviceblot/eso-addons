select
	dependency_dir missing_dir,
	required_by,
	a.id option_id,
	a.name option_name
from (
select
	adp.dependency_dir,
	group_concat(a.name, ', ') required_by
from installed_addon i
	inner join addon_dependency adp on i.addon_id = adp.addon_id
	inner join addon a on i.addon_id = a.id
where
	adp.dependency_dir not in (
		SELECT
			DISTINCT ad.dir
		FROM
			installed_addon i2
			inner join addon_dir ad on i2.addon_id = ad.addon_id
	)
group by
	adp.dependency_dir
)
left outer join addon_dir ad on dependency_dir = ad.dir
left outer join addon a on ad.addon_id = a.id
left outer join manual_dependency m on dependency_dir = m.addon_dir
where
	m.addon_dir is NULL
	or m.ignore <> 1