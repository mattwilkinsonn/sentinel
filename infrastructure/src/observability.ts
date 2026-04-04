import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import { cluster, logGroup, service } from "./backend";
import { stack } from "./config";
import { alb } from "./network";

// ---------------------------------------------------------------------------
// CloudWatch Observability
// ---------------------------------------------------------------------------
const alarmTopic = new aws.sns.Topic("sentinel-alarm-topic", {
  name: `sentinel-${stack}-alarms`,
});

const errNs = `Sentinel/${stack}`;
const errorFilter = new aws.cloudwatch.LogMetricFilter("app-error-filter", {
  name: `sentinel-${stack}-app-errors`,
  logGroupName: logGroup.name,
  pattern: '{ $.level = "ERROR" }',
  metricTransformation: {
    namespace: errNs,
    name: "AppErrors",
    value: "1",
    defaultValue: "0",
    unit: "Count",
  },
});

new aws.cloudwatch.MetricAlarm(
  "app-error-alarm",
  {
    name: `sentinel-${stack}-app-errors`,
    alarmDescription: "Backend logged an ERROR — check CloudWatch Logs",
    namespace: errNs,
    metricName: "AppErrors",
    statistic: "Sum",
    period: 60,
    evaluationPeriods: 1,
    threshold: 1,
    comparisonOperator: "GreaterThanOrEqualToThreshold",
    treatMissingData: "notBreaching",
    alarmActions: [alarmTopic.arn],
    okActions: [alarmTopic.arn],
  },
  { dependsOn: [errorFilter] },
);

const albArnSuffix = alb.arnSuffix;

new aws.cloudwatch.MetricAlarm("backend-5xx-alarm", {
  name: `sentinel-${stack}-5xx-errors`,
  alarmDescription: "Backend returning 5xx errors",
  namespace: "AWS/ApplicationELB",
  metricName: "HTTPCode_Target_5XX_Count",
  statistic: "Sum",
  period: 300,
  evaluationPeriods: 2,
  threshold: 10,
  comparisonOperator: "GreaterThanOrEqualToThreshold",
  treatMissingData: "notBreaching",
  dimensions: { LoadBalancer: albArnSuffix },
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
});

new aws.cloudwatch.MetricAlarm("backend-4xx-alarm", {
  name: `sentinel-${stack}-4xx-errors`,
  alarmDescription: "Elevated 4xx client errors",
  namespace: "AWS/ApplicationELB",
  metricName: "HTTPCode_Target_4XX_Count",
  statistic: "Sum",
  period: 300,
  evaluationPeriods: 3,
  threshold: 50,
  comparisonOperator: "GreaterThanOrEqualToThreshold",
  treatMissingData: "notBreaching",
  dimensions: { LoadBalancer: albArnSuffix },
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
});

new aws.cloudwatch.MetricAlarm("alb-5xx-alarm", {
  name: `sentinel-${stack}-alb-5xx`,
  alarmDescription: "ALB returning 5xx — targets may be down",
  namespace: "AWS/ApplicationELB",
  metricName: "HTTPCode_ELB_5XX_Count",
  statistic: "Sum",
  period: 60,
  evaluationPeriods: 3,
  threshold: 5,
  comparisonOperator: "GreaterThanOrEqualToThreshold",
  treatMissingData: "notBreaching",
  dimensions: { LoadBalancer: albArnSuffix },
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
});

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------
const region = aws.getRegion();

const dashboardBody = pulumi
  .all([
    albArnSuffix,
    cluster.name,
    service.name,
    pulumi.output(region).apply((r) => r.name),
  ])
  .apply(([albSuffix, ecsCluster, ecsSvc, reg]) => {
    return JSON.stringify({
      widgets: [
        {
          type: "metric",
          x: 0,
          y: 0,
          width: 12,
          height: 6,
          properties: {
            title: "HTTP 5xx / 4xx Errors",
            region: reg,
            metrics: [
              [
                "AWS/ApplicationELB",
                "HTTPCode_Target_5XX_Count",
                "LoadBalancer",
                albSuffix,
                { stat: "Sum", color: "#d62728", label: "5xx" },
              ],
              [
                "AWS/ApplicationELB",
                "HTTPCode_ELB_5XX_Count",
                "LoadBalancer",
                albSuffix,
                { stat: "Sum", color: "#9467bd", label: "ALB 5xx" },
              ],
              [
                "AWS/ApplicationELB",
                "HTTPCode_Target_4XX_Count",
                "LoadBalancer",
                albSuffix,
                { stat: "Sum", color: "#ff7f0e", label: "4xx" },
              ],
            ],
            period: 300,
            view: "timeSeries",
            stacked: false,
          },
        },
        {
          type: "metric",
          x: 12,
          y: 0,
          width: 12,
          height: 6,
          properties: {
            title: "Request Count",
            region: reg,
            metrics: [
              [
                "AWS/ApplicationELB",
                "RequestCount",
                "LoadBalancer",
                albSuffix,
                { stat: "Sum", label: "Requests" },
              ],
            ],
            period: 300,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 0,
          y: 6,
          width: 12,
          height: 6,
          properties: {
            title: "Response Time (p99 / avg)",
            region: reg,
            metrics: [
              [
                "AWS/ApplicationELB",
                "TargetResponseTime",
                "LoadBalancer",
                albSuffix,
                { stat: "p99", label: "p99" },
              ],
              [
                "AWS/ApplicationELB",
                "TargetResponseTime",
                "LoadBalancer",
                albSuffix,
                { stat: "Average", label: "avg" },
              ],
            ],
            period: 300,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 12,
          y: 6,
          width: 12,
          height: 6,
          properties: {
            title: "Healthy / Unhealthy Targets",
            region: reg,
            metrics: [
              [
                "AWS/ApplicationELB",
                "HealthyHostCount",
                "LoadBalancer",
                albSuffix,
                { stat: "Average", color: "#2ca02c", label: "Healthy" },
              ],
              [
                "AWS/ApplicationELB",
                "UnHealthyHostCount",
                "LoadBalancer",
                albSuffix,
                { stat: "Average", color: "#d62728", label: "Unhealthy" },
              ],
            ],
            period: 60,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 0,
          y: 12,
          width: 12,
          height: 6,
          properties: {
            title: "ECS CPU Utilization",
            region: reg,
            metrics: [
              [
                "AWS/ECS",
                "CPUUtilization",
                "ClusterName",
                ecsCluster,
                "ServiceName",
                ecsSvc,
                { stat: "Average", label: "CPU %" },
              ],
            ],
            period: 300,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 12,
          y: 12,
          width: 12,
          height: 6,
          properties: {
            title: "ECS Memory Utilization",
            region: reg,
            metrics: [
              [
                "AWS/ECS",
                "MemoryUtilization",
                "ClusterName",
                ecsCluster,
                "ServiceName",
                ecsSvc,
                { stat: "Average", label: "Memory %" },
              ],
            ],
            period: 300,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 0,
          y: 18,
          width: 24,
          height: 6,
          properties: {
            title: "Application Errors (ERROR log level)",
            region: reg,
            metrics: [
              [
                errNs,
                "AppErrors",
                { stat: "Sum", color: "#d62728", label: "Errors" },
              ],
            ],
            period: 60,
            view: "timeSeries",
          },
        },
      ],
    });
  });

new aws.cloudwatch.Dashboard("sentinel-dashboard", {
  dashboardName: `sentinel-${stack}`,
  dashboardBody,
});
